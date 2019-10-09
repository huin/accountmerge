use std::fmt;
use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
use failure::Error;
use ledger_parser::{Amount, Balance, Commodity, CommodityPosition, Posting, Transaction};
use regex::Regex;
use rust_decimal::Decimal;
use serde::{de, de::DeserializeOwned, Deserialize, Deserializer};
use sha1::{Digest, Sha1};

use crate::accounts::{EXPENSES_UNKNOWN, INCOME_UNKNOWN};
use crate::comment::Comment;
use crate::tags::{ACCOUNT_TAG, BANK_TAG, FINGERPRINT_TAG_PREFIX};

/// Transaction type field, provided by the bank.
pub const TRANSACTION_TYPE_TAG: &str = "trn_type";

const BANK_NAME: &str = "Nationwide";

#[derive(Debug, Fail)]
enum ReadError {
    #[fail(display = "bad file format: {}", reason)]
    BadFileFormat { reason: &'static str },
    #[fail(display = "bad header record, want {:?}, got {:?}", want, got)]
    BadHeaderRecord { want: &'static str, got: String },
}

impl ReadError {
    fn bad_file_format(reason: &'static str) -> ReadError {
        ReadError::BadFileFormat { reason }
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
    amount: DeGbpValue,
}

pub fn transactions_from_path<P: AsRef<Path>>(path: P) -> Result<Vec<Transaction>, Error> {
    let reader = encoding_rs_io::DecodeReaderBytesBuilder::new()
        .encoding(Some(encoding_rs::WINDOWS_1252))
        .build(File::open(path)?);
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

    let fp_key = {
        let mut hasher = Sha1::new();
        hasher.input(BANK_NAME);
        hasher.input("\0");
        hasher.input(&account_name);
        hasher.input("\0");
        let fingerprint = hasher.result_reset();
        let fingerprint_b64 = base64::encode_config(&fingerprint, base64::STANDARD_NO_PAD);
        format!("{}-{}", FINGERPRINT_TAG_PREFIX, &fingerprint_b64[0..8])
    };

    let mut self_hasher = Sha1::new();
    let mut peer_hasher = Sha1::new();

    let mut transactions = Vec::new();
    for result in csv_records {
        let str_record = result?;
        let record: DeTransaction = str_record.deserialize(None)?;

        record.hash(&mut self_hasher);
        peer_hasher.clone_from(&self_hasher);

        let self_account = "assets:unknown".to_string();
        let (peer_account, self_amt, peer_amt) = match (record.paid_in, record.paid_out) {
            // Paid in only.
            (Some(DeGbpValue(amt)), None) => {
                let peer_amt = neg_amount(&amt);
                (INCOME_UNKNOWN, amt, peer_amt)
            }
            // Paid out only.
            (None, Some(DeGbpValue(amt))) => (EXPENSES_UNKNOWN, neg_amount(&amt), amt),
            // Paid in and out or neither - both are errors.
            _ => {
                return Err(
                    ReadError::bad_file_format("expected either paid in or paid out").into(),
                )
            }
        };

        let self_fingerprint = base64::encode_config(
            &{
                self_hasher.input(&self_account);
                self_hasher.input("\0");
                self_hasher.input(self_amt.to_string());
                self_hasher.input("\0");
                self_hasher.result_reset()
            },
            base64::STANDARD_NO_PAD,
        );

        let mut self_comment = Comment::builder()
            .with_value_tag(ACCOUNT_TAG, account_name)
            .with_value_tag(BANK_TAG, BANK_NAME)
            .with_value_tag(TRANSACTION_TYPE_TAG, record.type_.clone())
            .build();

        let mut peer_comment = self_comment.clone();

        self_comment
            .value_tags
            .insert(fp_key.clone(), self_fingerprint);

        let peer_fingerprint = base64::encode_config(
            &{
                peer_hasher.input(&peer_account);
                peer_hasher.input("\0");
                peer_hasher.input(peer_amt.to_string());
                peer_hasher.input("\0");
                peer_hasher.result_reset()
            },
            base64::STANDARD_NO_PAD,
        );

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
                    account: self_account,
                    amount: self_amt,
                    balance: Some(Balance::Amount(record.balance.0)),
                    comment: self_comment.to_opt_comment(),
                    status: None,
                },
                Posting {
                    account: peer_account.to_string(),
                    amount: peer_amt,
                    balance: None,
                    comment: peer_comment.to_opt_comment(),
                    status: None,
                },
            ],
        });
    }

    Ok(transactions)
}

fn neg_amount(amt: &Amount) -> Amount {
    Amount {
        quantity: -amt.quantity,
        commodity: amt.commodity.clone(),
    }
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

impl DeTransaction {
    fn hash(&self, hasher: &mut Sha1) {
        hasher.input(&self.type_);
        hasher.input("\0");
        hasher.input(&self.date.0.format("%Y/%m/%d").to_string());
        hasher.input("\0");
        hasher.input(&self.description);
        hasher.input("\0");
        hasher.input(&self.balance.0.to_string());
        hasher.input("\0");
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

fn check_header(want: &'static str, got: &str) -> Result<(), ReadError> {
    if want != got {
        Err(ReadError::BadHeaderRecord {
            want,
            got: got.to_string(),
        }
        .into())
    } else {
        Ok(())
    }
}

fn deserialize_required_record<T, R>(
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
