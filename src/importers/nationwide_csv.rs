use chrono::NaiveDate;
use failure::Error;
use ledger_parser::{Amount, Balance, Posting, Transaction};
use structopt::StructOpt;

use crate::accounts::ASSETS_UNKNOWN;
use crate::comment::Comment;
use crate::filespec::FileSpec;
use crate::fingerprint::{make_prefix, FingerprintBuilder};
use crate::importers::importer::TransactionImporter;
use crate::importers::nationwide::{CommonOpts, BANK_NAME};
use crate::importers::nationwide_csv::de::*;
use crate::importers::util::{negate_amount, self_and_peer_account_amount};
use crate::tags::{ACCOUNT_TAG, BANK_TAG, IMPORT_PEER_TAG, IMPORT_SELF_TAG, UNKNOWN_ACCOUNT_TAG};

/// Transaction type field, provided by the bank.
pub const TRANSACTION_TYPE_TAG: &str = "trn_type";

#[derive(Debug, Fail)]
enum ReadError {
    #[fail(display = "bad file format: {}", reason)]
    FileFormat { reason: &'static str },
    #[fail(display = "bad header record, want {:?}, got {:?}", want, got)]
    HeaderRecord { want: &'static str, got: String },
}

impl ReadError {
    fn bad_file_format(reason: &'static str) -> ReadError {
        ReadError::FileFormat { reason }
    }
}

#[derive(Debug, Deserialize)]
struct AccountName {
    header: String,
    account_name: String,
}

#[derive(Debug, Deserialize)]
struct AccountQuantity {
    header: String,
    amount: GbpValue,
}

#[derive(Debug, StructOpt)]
/// Converts from Nationwide (nationwide.co.uk) CSV format to Ledger
/// transactions.
pub struct NationwideCsv {
    /// Nationwide CSV file to read from. "-" reads from stdin.
    input: FileSpec,

    #[structopt(flatten)]
    commonopts: CommonOpts,
}

impl TransactionImporter for NationwideCsv {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        let reader = encoding_rs_io::DecodeReaderBytesBuilder::new()
            .encoding(Some(encoding_rs::WINDOWS_1252))
            .build(self.input.reader()?);
        let mut csv_rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(reader);
        let mut csv_records = csv_rdr.records();

        let acct_name: AccountName = deserialize_required_record(&mut csv_records)?
            .ok_or_else(|| ReadError::bad_file_format("missing account name"))?;
        check_header("Account Name:", &acct_name.header)?;
        let balance: AccountQuantity = deserialize_required_record(&mut csv_records)?
            .ok_or_else(|| ReadError::bad_file_format("missing account balance"))?;
        check_header("Account Balance:", &balance.header)?;
        let available: AccountQuantity = deserialize_required_record(&mut csv_records)?
            .ok_or_else(|| ReadError::bad_file_format("missing available balance"))?;
        check_header("Available Balance:", &available.header)?;

        let fp_prefix = make_prefix(&self.commonopts.fp_prefix.to_prefix(&acct_name.account_name));

        read_transactions(&mut csv_records, &fp_prefix, &acct_name.account_name)
    }
}

fn read_transactions<R: std::io::Read>(
    csv_records: &mut csv::StringRecordsIter<R>,
    fp_prefix: &str,
    account_name: &str,
) -> Result<Vec<Transaction>, Error> {
    let headers: Vec<String> = deserialize_required_record(csv_records)?
        .ok_or_else(|| ReadError::bad_file_format("missing transaction headers"))?;
    if headers.len() != 6 {
        return Err(ReadError::bad_file_format("expected 6 headers for transactions").into());
    }
    check_header("Date", &headers[0])?;
    check_header("Transaction type", &headers[1])?;
    check_header("Description", &headers[2])?;
    check_header("Paid out", &headers[3])?;
    check_header("Paid in", &headers[4])?;
    check_header("Balance", &headers[5])?;

    let mut transactions = Vec::new();

    let mut prev_date: Option<NaiveDate> = None;
    let mut date_counter: i32 = 0;

    for result in csv_records {
        let str_record = result?;
        let mut record: Record = str_record.deserialize(None)?;
        let mut description = String::new();
        std::mem::swap(&mut description, &mut record.description);
        let date = record.date.0;

        // Maintain the per-date counter. Include a sequence number to each
        // transaction in a given day for use in the fingerprint.
        if Some(record.date.0) != prev_date {
            prev_date = Some(record.date.0);
            date_counter = 0;
        } else {
            date_counter += 1;
        }

        let (post1, post2) = form_postings(record, fp_prefix, account_name, date_counter)?;

        transactions.push(Transaction {
            date,
            description,
            comment: None,
            status: None,
            code: None,
            effective_date: None,
            postings: vec![post1, post2],
        });
    }

    Ok(transactions)
}

fn form_postings(
    record: Record,
    fp_prefix: &str,
    account_name: &str,
    date_counter: i32,
) -> Result<(Posting, Posting), Error> {
    let record_fpb = FingerprintBuilder::new().with(&record).with(date_counter);

    let self_amount: Amount = match (record.paid_in, record.paid_out) {
        // Paid in only.
        (Some(GbpValue(amt)), None) => amt,
        // Paid out only.
        (None, Some(GbpValue(amt))) => negate_amount(amt),
        // Paid in and out or neither - both are errors.
        _ => return Err(ReadError::bad_file_format("expected either paid in or paid out").into()),
    };

    let halves = self_and_peer_account_amount(self_amount, ASSETS_UNKNOWN.to_string());

    let self_fingerprint = record_fpb
        .clone()
        .with(halves.self_.account.as_str())
        .with(&halves.self_.amount)
        .build_with_prefix(&fp_prefix);

    let peer_fingerprint = record_fpb
        .with(halves.peer.account.as_str())
        .with(&halves.peer.amount)
        .build_with_prefix(&fp_prefix);

    let mut self_comment = Comment::builder()
        .with_tag(UNKNOWN_ACCOUNT_TAG)
        .with_value_tag(ACCOUNT_TAG, account_name)
        .with_value_tag(BANK_TAG, BANK_NAME)
        .with_value_tag(TRANSACTION_TYPE_TAG, record.type_.clone())
        .build();

    let mut peer_comment = self_comment.clone();

    self_comment.tags.insert(self_fingerprint);
    self_comment.tags.insert(IMPORT_SELF_TAG.to_string());
    peer_comment.tags.insert(peer_fingerprint);
    peer_comment.tags.insert(IMPORT_PEER_TAG.to_string());

    Ok((
        Posting {
            account: halves.self_.account,
            amount: halves.self_.amount,
            balance: Some(Balance::Amount(record.balance.0)),
            comment: self_comment.into_opt_comment(),
            status: None,
        },
        Posting {
            account: halves.peer.account,
            amount: halves.peer.amount,
            balance: None,
            comment: peer_comment.into_opt_comment(),
            status: None,
        },
    ))
}

mod de {
    use std::fmt;
    use std::str::FromStr;

    use chrono::NaiveDate;
    use failure::Error;
    use ledger_parser::{Amount, Commodity, CommodityPosition};
    use regex::Regex;
    use rust_decimal::Decimal;
    use serde::de::{self, Deserialize, DeserializeOwned, Deserializer};

    use super::ReadError;
    use crate::fingerprint::{FingerprintBuilder, Fingerprintable};

    #[derive(Debug, Deserialize)]
    pub struct Record {
        pub date: Date,
        pub type_: String,
        pub description: String,
        pub paid_out: Option<GbpValue>,
        pub paid_in: Option<GbpValue>,
        pub balance: GbpValue,
    }

    impl Fingerprintable for &Record {
        fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
            fpb.with(self.type_.as_str())
                .with(self.date.0)
                .with(self.description.as_str())
                .with(&self.balance.0)
        }
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
            formatter.write_str("a date string in \"DD Jan YYYY\" format")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            NaiveDate::parse_from_str(s, "%d %b %Y")
                .map(Date)
                .map_err(de::Error::custom)
        }
    }

    #[derive(Debug)]
    pub struct GbpValue(pub Amount);

    impl<'de> Deserialize<'de> for GbpValue {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_str(GbpValueVisitor)
        }
    }

    struct GbpValueVisitor;
    impl<'de> de::Visitor<'de> for GbpValueVisitor {
        type Value = GbpValue;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a monetary value string £NNN.NN format")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            lazy_static! {
                static ref RE: Regex = Regex::new(r"£(\d+)\.(\d+)").unwrap();
            }
            let captures = RE
                .captures(s)
                .ok_or_else(|| de::Error::custom("incorrect monetary format"))?;
            let pounds: i64 = deserialize_captured_number(&captures, 1)?;
            let pence: i64 = deserialize_captured_number(&captures, 2)?;
            Ok(GbpValue(Amount {
                commodity: Commodity {
                    name: "GBP".to_string(),
                    position: CommodityPosition::Left,
                },
                quantity: Decimal::new(pounds * 100 + pence, 2),
            }))
        }
    }

    pub fn check_header(want: &'static str, got: &str) -> Result<(), Error> {
        if want != got {
            Err(ReadError::HeaderRecord {
                want,
                got: got.to_string(),
            }
            .into())
        } else {
            Ok(())
        }
    }

    fn deserialize_captured_number<T, E>(c: &regex::Captures, i: usize) -> Result<T, E>
    where
        T: FromStr,
        E: de::Error,
        <T as FromStr>::Err: fmt::Display,
    {
        c.get(i)
            .unwrap()
            .as_str()
            .parse()
            .map_err(de::Error::custom)
    }

    pub fn deserialize_required_record<T, R>(
        csv_records: &mut csv::StringRecordsIter<R>,
    ) -> Result<Option<T>, Error>
    where
        T: DeserializeOwned,
        R: std::io::Read,
    {
        match csv_records.next() {
            Some(Ok(str_record)) => Ok(Some(str_record.deserialize(None)?)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::importers::nationwide::{CommonOpts, FpPrefix};
    use crate::importers::testutil::golden_test;

    use super::*;

    #[test]
    fn golden() {
        golden_test(
            &NationwideCsv {
                input: FileSpec::from_str("testdata/importers/nationwide_csv.csv").unwrap(),
                commonopts: CommonOpts {
                    fp_prefix: FpPrefix::Generated,
                },
            },
            "nationwide_csv.golden.journal",
        );
    }
}
