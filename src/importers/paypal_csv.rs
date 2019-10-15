use std::fs::File;
use std::path::Path;

use failure::Error;
use ledger_parser::Transaction;

use crate::importers::util::csv::{check_header, deserialize_required_record, ReadError};

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

    read_transactions(&mut csv_records)
}

fn read_transactions<R: std::io::Read>(
    csv_records: &mut csv::StringRecordsIter<R>,
) -> Result<Vec<Transaction>, Error> {
    let headers: Vec<String> = deserialize_required_record(csv_records)?
        .ok_or(ReadError::bad_file_format("missing transaction headers"))?;
    if headers.len() != 6 {
        return Err(ReadError::bad_file_format("expected 6 headers for transactions").into());
    }
    check_header("Date", &headers[0])?;
    check_header("Time", &headers[1])?;
    check_header("Time zone", &headers[2])?;
    check_header("Name", &headers[3])?;
    check_header("Type", &headers[4])?;
    check_header("Status", &headers[5])?;
    check_header("Currency", &headers[6])?;
    check_header("Amount", &headers[7])?;
    check_header("Receipt ID", &headers[8])?;
    check_header("Balance", &headers[9])?;

    let mut transactions = Vec::new();

    for result in csv_records {
        unimplemented!()
    }

    Ok(transactions)
}
