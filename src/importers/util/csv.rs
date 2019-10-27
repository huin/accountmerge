use std::fmt;
use std::str::FromStr;

use failure::Error;
use serde::de::{self, DeserializeOwned};

#[derive(Debug, Fail)]
pub enum ReadError {
    #[fail(display = "bad file format: {}", reason)]
    BadFileFormat { reason: &'static str },
    #[fail(display = "bad header record, want {:?}, got {:?}", want, got)]
    BadHeaderRecord { want: &'static str, got: String },
    #[fail(display = "invalid value for flag {}: {:?}", flag, value)]
    BadFlagValue { flag: &'static str, value: String },
}

impl ReadError {
    pub fn bad_file_format(reason: &'static str) -> ReadError {
        ReadError::BadFileFormat { reason }
    }
}

pub fn check_header(want: &'static str, got: &str) -> Result<(), ReadError> {
    if want != got {
        Err(ReadError::BadHeaderRecord {
            want,
            got: got.to_string(),
        })
    } else {
        Ok(())
    }
}

pub fn deserialize_captured_number<T, E>(c: &regex::Captures, i: usize) -> Result<T, E>
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
