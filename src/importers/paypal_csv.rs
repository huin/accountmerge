use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use clap::Args;
use itertools::Itertools;
use ledger_parser::{Amount, Balance, Commodity, CommodityPosition, Posting, Reality, Transaction};

use crate::accounts::ASSETS_UNKNOWN;
use crate::comment::Comment;
use crate::filespec::FileSpec;
use crate::fingerprint::FingerprintBuilder;
use crate::importers::importer::TransactionImporter;
use crate::importers::util::{self_and_peer_account_amount, self_and_peer_fingerprints};
use crate::ledgerutil::simple_posting_amount;
use crate::tags;
use crate::tzabbr::TzAbbrDB;

use super::importer::Import;

/// Transaction name field, provided by PayPal.
const TRANSACTION_NAME_TAG: &str = "trn_name";
/// Transaction type field, provided by PayPal.
const TRANSACTION_TYPE_TAG: &str = "trn_type";

#[derive(Debug, Args)]
/// Converts from PayPal CSV format to Ledger transactions.
pub struct PaypalCsv {
    /// PayPal CSV file to read from. "-" reads from stdin.
    input: FileSpec,
    /// Timezone of the output Ledger transactions.
    #[arg(long = "output-timezone")]
    output_timezone: Tz,
    #[arg(long = "fingerprint-namespace", default_value = "paypal")]
    // User namespace of the fingerprints to generate.
    fp_ns: String,
    /// Timezone abbreviations CSV file to use.
    timezone_abbr_file: FileSpec,
    /// Generate the legacy fingerprint tag.
    #[arg(long = "include-legacy-fingerprint")]
    include_legacy_fingerprint: bool,
}

impl TransactionImporter for PaypalCsv {
    fn get_transactions(&self) -> Result<Import> {
        let mut csv_rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(false)
            .trim(csv::Trim::All)
            .from_reader(self.input.reader()?);
        let headers = csv_rdr.headers()?.clone();
        let mut csv_records = csv_rdr.records();

        let tz_abbrs = TzAbbrDB::from_reader(self.timezone_abbr_file.reader()?)?;

        let transactions = self.read_transactions(&headers, &mut csv_records, &tz_abbrs)?;

        Ok(Import {
            user_fp_namespace: self.fp_ns.clone(),
            transactions,
        })
    }
}

impl PaypalCsv {
    fn read_transactions<R: std::io::Read>(
        &self,
        headers: &csv::StringRecord,
        csv_records: &mut csv::StringRecordsIter<R>,
        tz_abbrs: &TzAbbrDB,
    ) -> Result<Vec<Transaction>> {
        let records: Vec<Record> = csv_records
            .map(|row| deserialize_row(row, headers, tz_abbrs, &self.fp_ns))
            .collect::<Result<Vec<Record>>>()?;

        let record_groups = records.into_iter().group_by(|record| record.datetime);

        record_groups
            .into_iter()
            .map(|(dt, group)| self.form_transaction(dt, group.collect::<Vec<Record>>()))
            .collect::<Result<Vec<Transaction>>>()
    }

    fn form_transaction(
        &self,
        dt: DateTime<FixedOffset>,
        records: Vec<Record>,
    ) -> Result<Transaction> {
        let date = dt.with_timezone(&self.output_timezone).naive_local().date();

        let description = records
            .iter()
            .find_map(|record| record.name.clone())
            .ok_or_else(|| anyhow!("no record has a name for transactions at date time {}", dt))?;

        let mut postings = Vec::new();
        for (i, mut record) in records.into_iter().enumerate() {
            record.partial_fp = record.partial_fp.with(i);
            let (p1, p2) = self.form_postings(record);
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

    fn form_postings(&self, record: Record) -> (Posting, Posting) {
        let fp = self_and_peer_fingerprints(record.partial_fp);
        let self_comment = Comment::builder()
            .with_tag(tags::IMPORT_SELF)
            .with_tag(tags::UNKNOWN_ACCOUNT)
            .with_option_tag(if self.include_legacy_fingerprint {
                Some(fp.self_.legacy_tag())
            } else {
                None
            })
            .with_tag(fp.self_.tag())
            .build();
        let mut peer_comment = Comment::builder()
            .with_tag(tags::IMPORT_PEER)
            .with_tag(tags::UNKNOWN_ACCOUNT)
            .with_option_tag(if self.include_legacy_fingerprint {
                Some(fp.peer.legacy_tag())
            } else {
                None
            })
            .with_value_tag(TRANSACTION_TYPE_TAG, record.type_)
            .build();
        if let Some(name) = record.name {
            peer_comment
                .value_tags
                .insert(TRANSACTION_NAME_TAG.to_string(), name);
        }

        let halves = self_and_peer_account_amount(record.amount, ASSETS_UNKNOWN.to_string());

        let status = Some(record.status.into());

        (
            Posting {
                account: halves.self_.account,
                reality: Reality::Real,
                amount: Some(simple_posting_amount(halves.self_.amount)),
                balance: Some(Balance::Amount(record.balance)),
                comment: self_comment.into_opt_comment(),
                status,
            },
            Posting {
                account: halves.peer.account,
                reality: Reality::Real,
                amount: Some(simple_posting_amount(halves.peer.amount)),
                balance: None,
                comment: peer_comment.into_opt_comment(),
                status,
            },
        )
    }
}

struct Record {
    datetime: DateTime<FixedOffset>,
    name: Option<String>,
    type_: String,
    status: de::Status,
    amount: Amount,
    balance: Amount,
    partial_fp: FingerprintBuilder,
}

impl Record {
    fn from_csv_record(v: de::Record, tz_abbrs: &TzAbbrDB, fp_ns: &str) -> Result<Self> {
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
            commodity,
        };
        let partial_fp = FingerprintBuilder::new("ppcsv", 1, fp_ns)
            .with(v.date.0)
            .with(v.time.0)
            .with(v.time_zone.as_str())
            .with(v.name.as_deref())
            .with(v.type_.as_str())
            // Deliberately not including `v.status`, as this may change on a
            // future import.
            .with(&amount)
            .with(&balance);

        let naive_datetime = NaiveDateTime::new(v.date.0, v.time.0);

        let tz = parse_timezone(tz_abbrs, &v.time_zone)?;

        use chrono::LocalResult;
        let datetime: DateTime<FixedOffset> = match tz.from_local_datetime(&naive_datetime) {
            LocalResult::None => bail!(
                "nonexistant combination of date time {} and timezone: {}",
                naive_datetime,
                tz
            ),
            LocalResult::Ambiguous(_, _) => bail!(
                "ambiguous combination of date time {} and timezone: {}",
                naive_datetime,
                tz
            ),
            LocalResult::Single(dt) => dt,
        };
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
    tz_abbrs: &TzAbbrDB,
    fp_ns: &str,
) -> Result<Record> {
    let de_record: de::Record = sr?.deserialize(Some(headers))?;
    Record::from_csv_record(de_record, tz_abbrs, fp_ns)
}

mod de {
    use std::fmt;

    use chrono::{NaiveDate, NaiveTime};

    use ledger_parser::TransactionStatus;
    use rust_decimal::Decimal;
    use serde::{de, Deserialize, Deserializer};
    use serde_derive::Deserialize;

    #[derive(Deserialize)]
    pub struct Record {
        #[serde(rename = "Date")]
        pub date: Date,
        #[serde(rename = "Time")]
        pub time: Time,
        #[serde(rename = "Time zone")]
        pub time_zone: String,
        #[serde(rename = "Name")]
        pub name: Option<String>,
        #[serde(rename = "Type")]
        pub type_: String,
        #[serde(rename = "Status")]
        pub status: Status,
        #[serde(rename = "Currency")]
        pub currency: String,
        #[serde(rename = "Amount")]
        pub amount: Decimal,
        #[serde(rename = "Receipt ID")]
        pub receipt_id: Option<String>,
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

    #[allow(clippy::from_over_into)] // Can't implement `From for TransactionStatus` from other crate.
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

fn parse_timezone(tz_abbr: &TzAbbrDB, s: &str) -> Result<FixedOffset> {
    if let Some(tz) = tz_abbr.abbr_to_tz(s) {
        return Ok(tz);
    }
    bail!("unknown timezone {:?}", s);
}

#[cfg(test)]
mod tests {
    use crate::importers::testutil::golden_test;
    use std::str::FromStr;

    use super::*;

    #[test]
    fn golden() {
        golden_test(
            &PaypalCsv {
                input: FileSpec::from_str("testdata/importers/paypal_csv.csv").unwrap(),
                output_timezone: Tz::UTC,
                fp_ns: "paypal".to_string(),
                timezone_abbr_file: FileSpec::from_str(
                    "testdata/importers/paypal_csv_tz_abbrs.csv",
                )
                .unwrap(),
                include_legacy_fingerprint: true,
            },
            "paypal_csv.golden.journal",
        );
    }
}
