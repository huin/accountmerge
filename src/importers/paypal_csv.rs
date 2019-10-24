use std::convert::{TryFrom, TryInto};
use std::fmt::Display;
use std::path::PathBuf;
use std::str::FromStr;

use chrono::{DateTime, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use failure::Error;
use itertools::Itertools;
use ledger_parser::{Amount, Balance, Commodity, CommodityPosition, Posting, Transaction};
use structopt::StructOpt;

use crate::comment::Comment;
use crate::fingerprint::FingerprintBuilder;
use crate::importers::importer::TransactionImporter;

/// Transaction name field, provided by PayPal.
const TRANSACTION_NAME_TAG: &str = "trn_name";
/// Transaction type field, provided by PayPal.
const TRANSACTION_TYPE_TAG: &str = "trn_type";

#[derive(Debug, Fail)]
enum ReadError {
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
    #[fail(display = "unknown timezone {:?}", timezone)]
    UnknownTimezone { timezone: String },
}

#[derive(Debug)]
struct TzDisplay(pub Tz);

impl Display for TzDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.write_str(self.0.name())
    }
}

#[derive(Debug, StructOpt)]
pub struct PaypalCsv {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
    output_timezone: Tz,
    #[structopt(long = "fingerprint-key", default_value = "paypal")]
    fingerprint_key: String,
}

impl TransactionImporter for PaypalCsv {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        let mut csv_rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(false)
            .trim(csv::Trim::All)
            .from_path(&self.input)?;
        let headers = csv_rdr.headers()?.clone();
        let mut csv_records = csv_rdr.records();

        self.read_transactions(&headers, &mut csv_records)
    }
}

impl PaypalCsv {
    fn read_transactions<R: std::io::Read>(
        &self,
        headers: &csv::StringRecord,
        csv_records: &mut csv::StringRecordsIter<R>,
    ) -> Result<Vec<Transaction>, Error> {
        let records: Vec<Record> = csv_records
            .map(|row| deserialize_row(row, headers))
            .collect::<Result<Vec<Record>, Error>>()?;

        let record_groups = records.into_iter().group_by(|record| record.datetime);

        let fp_key = format!(
            "{}{}",
            crate::tags::FINGERPRINT_TAG_PREFIX,
            self.fingerprint_key
        );

        record_groups
            .into_iter()
            .map(|(dt, group)| self.form_transaction(dt, group.collect::<Vec<Record>>(), &fp_key))
            .collect::<Result<Vec<Transaction>, Error>>()
    }

    fn form_transaction(
        &self,
        dt: DateTime<Tz>,
        records: Vec<Record>,
        fp_key: &str,
    ) -> Result<Transaction, Error> {
        let date = dt.with_timezone(&self.output_timezone).naive_local().date();

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
            let (p1, p2) = form_postings(record, fp_key);
            postings.push(p1);
            postings.push(p2);
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
}

fn form_postings(record: Record, fp_key: &str) -> (Posting, Posting) {
    let self_comment = Comment::builder()
        .with_value_tag(fp_key, record.partial_fp.clone().with_str("self").build())
        .build();
    let mut peer_comment = Comment::builder()
        .with_value_tag(fp_key, record.partial_fp.with_str("peer").build())
        .with_value_tag(TRANSACTION_TYPE_TAG, record.type_)
        .build();
    if !record.name.is_empty() {
        peer_comment
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

    let status = Some(record.status.into());

    (
        Posting {
            account: self_account.to_string(),
            amount: self_amount,
            balance: Some(Balance::Amount(record.balance)),
            comment: self_comment.to_opt_comment(),
            status: status.clone(),
        },
        Posting {
            account: peer_account.to_string(),
            amount: peer_amount,
            balance: None,
            comment: peer_comment.to_opt_comment(),
            status: status,
        },
    )
}

struct Record {
    datetime: DateTime<Tz>,
    name: String,
    type_: String,
    status: de::Status,
    amount: Amount,
    balance: Amount,
    partial_fp: FingerprintBuilder,
}

impl TryFrom<de::Record> for Record {
    type Error = Error;
    fn try_from(v: de::Record) -> Result<Self, Error> {
        let commodity = Commodity {
            name: v.currency,
            position: CommodityPosition::Left,
        };
        let amount = Amount {
            quantity: v.amount,
            commodity: commodity.clone(),
        };
        let balance = Amount {
            quantity: v.balance,
            commodity: commodity,
        };
        let partial_fp = FingerprintBuilder::new()
            .with_naive_date(&v.date.0)
            .with_naive_time(&v.time.0)
            .with_str(&v.time_zone)
            .with_str(&v.name)
            .with_str(&v.type_)
            // Deliberately not including `v.status`, as this may change on a
            // future import.
            .with_amount(&amount)
            .with_amount(&balance);

        let naive_datetime = chrono::NaiveDateTime::new(v.date.0, v.time.0);

        let tz = parse_timezone(&v.time_zone)?;

        use chrono::LocalResult;
        let datetime: DateTime<Tz> = match tz.from_local_datetime(&naive_datetime) {
            LocalResult::None => Err(ReadError::NonexistantTime {
                datetime: naive_datetime,
                timezone: TzDisplay(tz),
            }),
            LocalResult::Ambiguous(_, _) => Err(ReadError::AmbiguousTime {
                datetime: naive_datetime,
                timezone: TzDisplay(tz),
            }),
            LocalResult::Single(dt) => Ok(dt),
        }?;
        Ok(Self {
            datetime,
            name: v.name,
            type_: v.type_,
            status: v.status,
            amount,
            balance,
            partial_fp,
        })
    }
}

fn deserialize_row(
    sr: csv::Result<csv::StringRecord>,
    headers: &csv::StringRecord,
) -> Result<Record, Error> {
    let de_record: de::Record = sr?.deserialize(Some(headers))?;
    de_record.try_into()
}

mod de {
    use std::fmt;

    use chrono::{NaiveDate, NaiveTime};

    use ledger_parser::TransactionStatus;
    use rust_decimal::Decimal;
    use serde::{de, Deserialize, Deserializer};

    #[derive(Deserialize)]
    pub struct Record {
        #[serde(rename = "Date")]
        pub date: Date,
        #[serde(rename = "Time")]
        pub time: Time,
        #[serde(rename = "Time zone")]
        pub time_zone: String,
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

    #[derive(Clone, Copy, Debug, Deserialize)]
    pub enum Status {
        Completed,
        Pending,
    }

    impl Into<TransactionStatus> for Status {
        fn into(self) -> TransactionStatus {
            use Status::*;
            match self {
                Completed => TransactionStatus::Cleared,
                Pending => TransactionStatus::Pending,
            }
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

fn parse_timezone(s: &str) -> Result<Tz, Error> {
    if let Some(tz) = parse_timezone_abbr(s) {
        return Ok(tz);
    }
    <Tz as FromStr>::from_str(s).map_err(|_| {
        ReadError::UnknownTimezone {
            timezone: s.to_string(),
        }
        .into()
    })
}

fn parse_timezone_abbr(s: &str) -> Option<Tz> {
    use Tz::*;
    // TODO: Need a better database of timezone abbreviations.
    match s {
        "BST" => Some(Etc__GMTPlus1),
        _ => None,
    }
}
