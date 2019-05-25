use std::fmt;
use std::fs::File;
use std::path::Path;

use chrono::NaiveDate;
use failure::Error;
use regex::Regex;
use serde::{de, de::DeserializeOwned, Deserialize, Deserializer};

use crate::bank::{InputTransaction,Paid};
use crate::money::GbpValue;

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

pub fn transactions_from_path<P: AsRef<Path>>(path: P) -> Result<Vec<InputTransaction>, Error> {
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
    ReadError::check_header("Account Name:", &acct_name.header)?;
    let balance: AccountQuantity = deserialize_required_record(&mut csv_records)?
        .ok_or(ReadError::bad_file_format("missing account balance"))?;
    ReadError::check_header("Account Balance:", &balance.header)?;
    let available: AccountQuantity = deserialize_required_record(&mut csv_records)?
        .ok_or(ReadError::bad_file_format("missing available balance"))?;
    ReadError::check_header("Available Balance:", &available.header)?;

    read_transactions(&acct_name.account_name, &mut csv_records)
}

fn read_transactions<R: std::io::Read>(
    account_name: &str,
    csv_records: &mut csv::StringRecordsIter<R>,
) -> Result<Vec<InputTransaction>, Error> {
    let headers: Vec<String> = deserialize_required_record(csv_records)?
        .ok_or(ReadError::bad_file_format("missing transaction headers"))?;
    if headers.len() != 6 {
        return Err(ReadError::bad_file_format("expected 6 headers for transactions").into());
    }
    ReadError::check_header("Date", &headers[0])?;
    ReadError::check_header("Transaction type", &headers[1])?;
    ReadError::check_header("Description", &headers[2])?;
    ReadError::check_header("Paid out", &headers[3])?;
    ReadError::check_header("Paid in", &headers[4])?;
    ReadError::check_header("Balance", &headers[5])?;

    let mut transactions = Vec::new();
    for result in csv_records {
        let str_record = result?;
        let record: DeTransaction = str_record.deserialize(None)?;
        transactions.push(InputTransaction{
            src_bank: "nationwide".to_string(),
            src_acct: account_name.to_string(),
            date: record.date.0,
            type_: record.type_,
            description: record.description,
            paid: match (record.paid_in, record.paid_out) {
                    (Some(DeGbpValue(v)), None) => Paid::In(v),
                    (None, Some(DeGbpValue(v))) => Paid::Out(v),
                    _ => return Err(ReadError::bad_file_format("expected either paid in or paid out").into()),
            },
            balance: record.balance.0,
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
struct DeGbpValue(GbpValue);

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
        let pounds: i32 = deserialize_captured_number(&captures, 1)?;
        let pence: i32 = deserialize_captured_number(&captures, 2)?;
        Ok(DeGbpValue(GbpValue {
            pence: pounds * 100 + pence,
        }))
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

fn deserialize_captured_number<E: de::Error>(c: &regex::Captures, i: usize) -> Result<i32, E> {
    c.get(i)
        .unwrap()
        .as_str()
        .parse()
        .map_err(de::Error::custom)
}
