use std::fmt;
use std::fs::File;
use std::path::Path;

use chrono::NaiveDate;
use failure::Error;
use regex::Regex;
use serde::{de, de::DeserializeOwned, Deserialize, Deserializer};

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
    amount: GbpValue,
}

pub struct Statement {
    pub account_name: String,
    pub closing_balance: GbpValue,
    pub available_balance: GbpValue,
    pub transactions: Vec<Transaction>,
}

impl Statement {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Statement, Error> {
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

        let transactions = Self::read_transactions(&mut csv_records)?;

        Ok(Statement {
            account_name: acct_name.account_name,
            closing_balance: balance.amount,
            available_balance: available.amount,
            transactions,
        })
    }

    fn read_transactions<R: std::io::Read>(
        csv_records: &mut csv::StringRecordsIter<R>,
    ) -> Result<Vec<Transaction>, Error> {
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
            let record: Transaction = str_record.deserialize(None)?;
            transactions.push(record);
        }
        Ok(transactions)
    }
}

#[derive(Debug, Deserialize)]
pub struct Transaction {
    pub date: InputDate,
    pub type_: String,
    pub description: String,
    pub paid_out: Option<GbpValue>,
    pub paid_in: Option<GbpValue>,
    pub balance: GbpValue,
}

#[derive(Debug)]
pub struct InputDate(pub NaiveDate);

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

pub struct GbpValue {
    pub pence: i32,
}

impl GbpValue {
    pub fn parts(&self) -> (i32, i32) {
        (self.pence / 100, self.pence % 100)
    }
}

impl fmt::Debug for GbpValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let parts = self.parts();
        write!(f, "GbpValue({}.{})", parts.0, parts.1)
    }
}

impl fmt::Display for GbpValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let parts = self.parts();
        write!(f, "GBP {}.{}", parts.0, parts.1)
    }
}

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
        let pounds: i32 = deserialize_captured_number(&captures, 1)?;
        let pence: i32 = deserialize_captured_number(&captures, 2)?;
        Ok(GbpValue {
            pence: pounds * 100 + pence,
        })
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
