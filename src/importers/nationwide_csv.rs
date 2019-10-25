use std::fmt;
use std::fs::File;
use std::path::PathBuf;

use chrono::NaiveDate;
use failure::Error;
use ledger_parser::{Amount, Balance, Commodity, CommodityPosition, Posting, Transaction};
use regex::Regex;
use rust_decimal::Decimal;
use serde::{de, Deserialize, Deserializer};
use structopt::StructOpt;

use crate::accounts::ASSETS_UNKNOWN;
use crate::comment::Comment;
use crate::fingerprint::{FingerprintBuilder, Fingerprintable};
use crate::importers::importer::TransactionImporter;
use crate::importers::util::csv::{
    check_header, deserialize_captured_number, deserialize_required_record, ReadError,
};
use crate::importers::util::{negate_amount, self_and_peer_account_amount};
use crate::tags::{ACCOUNT_TAG, BANK_TAG, FINGERPRINT_TAG_PREFIX, UNKNOWN_ACCOUNT_TAG};

/// Transaction type field, provided by the bank.
pub const TRANSACTION_TYPE_TAG: &str = "trn_type";

const BANK_NAME: &str = "Nationwide";

#[derive(Debug, Deserialize)]
struct AccountName {
    header: String,
    account_name: String,
}

#[derive(Debug, Deserialize)]
struct AccountQuantity {
    header: String,
    amount: DeGbpValue,
}

#[derive(Debug, StructOpt)]
/// Converts from Nationwide (nationwide.co.uk) CSV format to Ledger
/// transactions.
pub struct NationwideCsv {
    #[structopt(parse(from_os_str))]
    /// Nationwide CSV file to read from.
    input: PathBuf,
}

impl TransactionImporter for NationwideCsv {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        let reader = encoding_rs_io::DecodeReaderBytesBuilder::new()
            .encoding(Some(encoding_rs::WINDOWS_1252))
            .build(File::open(&self.input)?);
        let mut csv_rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(reader);
        let mut csv_records = csv_rdr.records();

        let acct_name: AccountName = deserialize_required_record(&mut csv_records)?
            .ok_or(ReadError::bad_file_format("missing account name"))?;
        check_header("Account Name:", &acct_name.header)?;
        let balance: AccountQuantity = deserialize_required_record(&mut csv_records)?
            .ok_or(ReadError::bad_file_format("missing account balance"))?;
        check_header("Account Balance:", &balance.header)?;
        let available: AccountQuantity = deserialize_required_record(&mut csv_records)?
            .ok_or(ReadError::bad_file_format("missing available balance"))?;
        check_header("Available Balance:", &available.header)?;

        read_transactions(&acct_name.account_name, &mut csv_records)
    }
}

fn read_transactions<R: std::io::Read>(
    account_name: &str,
    csv_records: &mut csv::StringRecordsIter<R>,
) -> Result<Vec<Transaction>, Error> {
    let headers: Vec<String> = deserialize_required_record(csv_records)?
        .ok_or(ReadError::bad_file_format("missing transaction headers"))?;
    if headers.len() != 6 {
        return Err(ReadError::bad_file_format("expected 6 headers for transactions").into());
    }
    check_header("Date", &headers[0])?;
    check_header("Transaction type", &headers[1])?;
    check_header("Description", &headers[2])?;
    check_header("Paid out", &headers[3])?;
    check_header("Paid in", &headers[4])?;
    check_header("Balance", &headers[5])?;

    let fp_key = FingerprintBuilder::new()
        .with_str(BANK_NAME)
        .with_str(&account_name)
        .build_with_prefix(FINGERPRINT_TAG_PREFIX);

    let mut transactions = Vec::new();

    let mut prev_date: Option<NaiveDate> = None;
    let mut date_counter: i32 = 0;

    for result in csv_records {
        let str_record = result?;
        let record: DeTransaction = str_record.deserialize(None)?;

        // Maintain the per-date counter. Include a sequence number to each
        // transaction in a given day for use in the fingerprint.
        if Some(record.date.0) != prev_date {
            prev_date = Some(record.date.0.clone());
            date_counter = 0;
        } else {
            date_counter += 1;
        }

        let record_fpb = FingerprintBuilder::new()
            .with_fingerprintable(&record)
            .with_i32(date_counter);

        let self_amount: Amount = match (record.paid_in, record.paid_out) {
            // Paid in only.
            (Some(DeGbpValue(amt)), None) => amt,
            // Paid out only.
            (None, Some(DeGbpValue(amt))) => negate_amount(amt),
            // Paid in and out or neither - both are errors.
            _ => {
                return Err(
                    ReadError::bad_file_format("expected either paid in or paid out").into(),
                )
            }
        };

        let halves = self_and_peer_account_amount(self_amount, ASSETS_UNKNOWN.to_string());

        let self_fingerprint = record_fpb
            .clone()
            .with_str(&halves.self_.account)
            .with_amount(&halves.self_.amount)
            .build();

        let peer_fingerprint = record_fpb
            .with_str(&halves.peer.account)
            .with_amount(&halves.peer.amount)
            .build();

        let mut self_comment = Comment::builder()
            .with_tag(UNKNOWN_ACCOUNT_TAG)
            .with_value_tag(ACCOUNT_TAG, account_name)
            .with_value_tag(BANK_TAG, BANK_NAME)
            .with_value_tag(TRANSACTION_TYPE_TAG, record.type_.clone())
            .build();

        let mut peer_comment = self_comment.clone();

        self_comment
            .value_tags
            .insert(fp_key.clone(), self_fingerprint);

        peer_comment
            .value_tags
            .insert(fp_key.clone(), peer_fingerprint);

        transactions.push(Transaction {
            date: record.date.0,
            description: record.description,
            comment: None,
            status: None,
            code: None,
            effective_date: None,
            postings: vec![
                Posting {
                    account: halves.self_.account,
                    amount: halves.self_.amount,
                    balance: Some(Balance::Amount(record.balance.0)),
                    comment: self_comment.to_opt_comment(),
                    status: None,
                },
                Posting {
                    account: halves.peer.account,
                    amount: halves.peer.amount,
                    balance: None,
                    comment: peer_comment.to_opt_comment(),
                    status: None,
                },
            ],
        });
    }

    Ok(transactions)
}

#[derive(Debug, Deserialize)]
struct DeTransaction {
    date: InputDate,
    type_: String,
    description: String,
    paid_out: Option<DeGbpValue>,
    paid_in: Option<DeGbpValue>,
    balance: DeGbpValue,
}

impl Fingerprintable for DeTransaction {
    fn fingerprint(&self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        fpb.with_str(&self.type_)
            .with_naive_date(&self.date.0)
            .with_str(&self.description)
            .with_amount(&self.balance.0)
    }
}

#[derive(Debug)]
struct InputDate(NaiveDate);

impl<'de> Deserialize<'de> for InputDate {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_str(InputDateVisitor)
    }
}

struct InputDateVisitor;
impl<'de> de::Visitor<'de> for InputDateVisitor {
    type Value = InputDate;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a date string in \"DD Jan YYYY\" format")
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
        NaiveDate::parse_from_str(s, "%d %b %Y")
            .map(InputDate)
            .map_err(de::Error::custom)
    }
}

#[derive(Debug)]
struct DeGbpValue(Amount);

impl<'de> Deserialize<'de> for DeGbpValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_str(DeGbpValueVisitor)
    }
}

struct DeGbpValueVisitor;
impl<'de> de::Visitor<'de> for DeGbpValueVisitor {
    type Value = DeGbpValue;

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
        Ok(DeGbpValue(Amount {
            commodity: Commodity {
                name: "GBP".to_string(),
                position: CommodityPosition::Left,
            },
            quantity: Decimal::new(pounds * 100 + pence, 2),
        }))
    }
}
