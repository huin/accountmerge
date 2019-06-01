use std::convert::TryInto;
use std::fmt;
use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use chrono::NaiveDate;
use failure::Error;
use regex::Regex;
use serde::{de, de::DeserializeOwned, Deserialize, Deserializer};

use crate::bank::{InputTransaction, Paid};
use crate::money::GbpValue;

const BANK_NAME: &'static str = "Nationwide";

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
) -> Result<Vec<InputTransaction>, Error> {
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

    let mut transactions = Vec::new();
    for result in csv_records {
        let str_record = result?;
        let record: DeTransaction = str_record.deserialize(None)?;
        transactions.push(InputTransaction {
            bank: BANK_NAME.to_string(),
            account_name: account_name.to_string(),
            date: record.date.0,
            type_: record.type_,
            description: record.description,
            paid: match (record.paid_in, record.paid_out) {
                (Some(DeGbpValue(v)), None) => Paid::In(v.try_into()?),
                (None, Some(DeGbpValue(v))) => Paid::Out(v.try_into()?),
                _ => {
                    return Err(
                        ReadError::bad_file_format("expected either paid in or paid out").into(),
                    )
                }
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
        Ok(DeGbpValue(GbpValue::from_parts(pounds, pence)))
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
