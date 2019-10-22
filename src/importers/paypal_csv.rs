use std::convert::{TryFrom, TryInto};
use std::fmt::Display;
use std::path::Path;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use failure::Error;
use itertools::Itertools;
use ledger_parser::{
    Amount, Balance, Commodity, CommodityPosition, Posting, Transaction, TransactionStatus,
};

use crate::comment::Comment;

/// Transaction name field, provided by PayPal.
pub const TRANSACTION_NAME_TAG: &str = "trn_name";
/// Transaction type field, provided by PayPal.
pub const TRANSACTION_TYPE_TAG: &str = "trn_type";

#[derive(Debug, Fail)]
pub enum ReadError {
    #[fail(
        display = "ambiguous combination of date time {} and timezone: {}",
        datetime, timezone
    )]
    AmbiguousTime {
        datetime: NaiveDateTime,
        timezone: TzDisplay,
    },
    #[fail(
        display = "nonexistant combination of date time {} and timezone: {}",
        datetime, timezone
    )]
    NonexistantTime {
        datetime: NaiveDateTime,
        timezone: TzDisplay,
    },
    #[fail(
        display = "no record has a name for transactions at date time {}",
        datetime
    )]
    NoNameForGroup { datetime: DateTime<Tz> },
}

#[derive(Debug)]
pub struct TzDisplay(pub Tz);

impl Display for TzDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.write_str(self.0.name())
    }
}

pub fn transactions_from_path<P: AsRef<Path>>(
    path: P,
    output_timezone: &Tz,
) -> Result<Vec<Transaction>, Error> {
    let mut csv_rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .trim(csv::Trim::All)
        .from_path(path)?;
    let headers = csv_rdr.headers()?.clone();
    let mut csv_records = csv_rdr.records();

    read_transactions(&headers, &mut csv_records, output_timezone)
}

// TODO: Pass in timezone as a Tz that transactions should be output in.
fn read_transactions<R: std::io::Read>(
    headers: &csv::StringRecord,
    csv_records: &mut csv::StringRecordsIter<R>,
    output_timezone: &Tz,
) -> Result<Vec<Transaction>, Error> {
    let records: Vec<Record> = csv_records
        .map(|row| deserialize_row(row, headers))
        .collect::<Result<Vec<Record>, Error>>()?;

    let record_groups = records.into_iter().group_by(|record| record.datetime);

    record_groups
        .into_iter()
        .map(|(dt, group)| form_transaction(dt, group.collect::<Vec<Record>>(), output_timezone))
        .collect::<Result<Vec<Transaction>, Error>>()
}

fn deserialize_row(
    sr: csv::Result<csv::StringRecord>,
    headers: &csv::StringRecord,
) -> Result<Record, Error> {
    let de_record: de::Record = sr?.deserialize(Some(headers))?;
    de_record.try_into()
}

fn form_transaction(
    dt: DateTime<Tz>,
    records: Vec<Record>,
    output_timezone: &Tz,
) -> Result<Transaction, Error> {
    let date = dt.with_timezone(output_timezone).naive_local().date();

    let description = records
        .iter()
        .find_map(|record| {
            if !record.name.is_empty() {
                Some(record.name.clone())
            } else {
                None
            }
        })
        .ok_or_else(|| ReadError::NoNameForGroup { datetime: dt })?;

    let mut postings = Vec::new();
    for record in records.into_iter() {
        let (p1, p2) = form_postings(record);
        postings.insert(p1);
        postings.insert(p2);
    }

    Ok(Transaction {
        description,
        code: None,
        comment: None,
        date,
        effective_date: None,
        status: None,
        postings,
    })
}

fn form_postings(record: Record) -> (Posting, Posting) {
    let mut comment = Comment::builder()
        .with_value_tag(TRANSACTION_TYPE_TAG, record.type_)
        .build();
    if !record.name.is_empty() {
        comment
            .value_tags
            .insert(TRANSACTION_NAME_TAG.to_string(), record.name);
    }

    let self_account = "assets:paypal";
    let peer_account = if record.amount.quantity.is_sign_negative() {
        "expenses:unknown"
    } else {
        "income:unknown"
    };

    let self_amount = record.amount.clone();
    let peer_amount = Amount {
        commodity: record.amount.commodity.clone(),
        quantity: -record.amount.quantity,
    };

    let status = Some(match record.status {
        Status::Completed => TransactionStatus::Cleared,
        Status::Pending => TransactionStatus::Pending,
    });

    (
        Posting {
            account: self_account.to_string(),
            amount: self_amount,
            balance: Some(Balance::Amount(record.balance)),
            comment: comment.to_opt_comment(),
            status: status.clone(),
        },
        Posting {
            account: peer_account.to_string(),
            amount: peer_amount,
            balance: Some(Balance::Amount(record.balance)),
            comment: comment.to_opt_comment(),
            status: status,
        },
    )
}

struct Record {
    datetime: DateTime<Tz>,
    name: String,
    type_: String,
    status: Status,
    amount: Amount,
    balance: Amount,
}

impl TryFrom<de::Record> for Record {
    type Error = Error;
    fn try_from(v: de::Record) -> Result<Self, Error> {
        use chrono::LocalResult;
        let naive_datetime = chrono::NaiveDateTime::new(v.date.0, v.time.0);
        let datetime: DateTime<Tz> = match v.time_zone.from_local_datetime(&naive_datetime) {
            LocalResult::None => Err(ReadError::NonexistantTime {
                datetime: naive_datetime,
                timezone: TzDisplay(v.time_zone),
            }),
            LocalResult::Ambiguous(_, _) => Err(ReadError::AmbiguousTime {
                datetime: naive_datetime,
                timezone: TzDisplay(v.time_zone),
            }),
            LocalResult::Single(dt) => Ok(dt),
        }?;
        let commodity = Commodity {
            name: v.currency,
            position: CommodityPosition::Left,
        };
        Ok(Self {
            datetime,
            name: v.name,
            type_: v.type_,
            status: v.status,
            amount: Amount {
                quantity: v.amount,
                commodity: commodity.clone(),
            },
            balance: Amount {
                quantity: v.balance,
                commodity,
            },
        })
    }
}

#[derive(Debug, Deserialize)]
pub enum Status {
    Completed,
    Pending,
}

mod de {
    use std::fmt;

    use chrono::{NaiveDate, NaiveTime};
    use rust_decimal::Decimal;
    use serde::{de, Deserialize, Deserializer};

    use super::Status;

    #[derive(Deserialize)]
    pub struct Record {
        #[serde(rename = "Date")]
        pub date: Date,
        #[serde(rename = "Time")]
        pub time: Time,
        #[serde(rename = "Time zone")]
        pub time_zone: chrono_tz::Tz,
        #[serde(rename = "Name")]
        pub name: String,
        #[serde(rename = "Type")]
        pub type_: String,
        #[serde(rename = "Status")]
        pub status: Status,
        #[serde(rename = "Currency")]
        pub currency: String,
        #[serde(rename = "Amount")]
        pub amount: Decimal,
        #[serde(rename = "Receipt ID")]
        pub receipt_id: String,
        #[serde(rename = "Balance")]
        pub balance: Decimal,
    }

    #[derive(Debug)]
    pub struct Date(pub NaiveDate);

    impl<'de> Deserialize<'de> for Date {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_str(DateVisitor)
        }
    }

    struct DateVisitor;
    impl<'de> de::Visitor<'de> for DateVisitor {
        type Value = Date;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a date string in \"DD/MM/YYYY\" format")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            NaiveDate::parse_from_str(s, "%d/%m/%Y")
                .map(Date)
                .map_err(de::Error::custom)
        }
    }

    #[derive(Debug)]
    pub struct Time(pub NaiveTime);

    impl<'de> Deserialize<'de> for Time {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_str(TimeVisitor)
        }
    }

    struct TimeVisitor;
    impl<'de> de::Visitor<'de> for TimeVisitor {
        type Value = Time;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a time string in \"HH:MM:SS\" format")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            NaiveTime::parse_from_str(s, "%H:%M:%S")
                .map(Time)
                .map_err(de::Error::custom)
        }
    }
}
