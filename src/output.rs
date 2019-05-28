use std::fmt;

use chrono::{Datelike, NaiveDate};

use crate::money::GbpValue;

pub struct Transaction {
    pub date: NaiveDate,
    pub description: String,
    pub postings: Vec<Posting>,
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        writeln!(
            f,
            "{:04}/{:02}/{:02} {}",
            self.date.year(),
            self.date.month(),
            self.date.day(),
            self.description
        )?;
        for p in &self.postings {
            writeln!(f, "  {}", p)?;
        }
        Ok(())
    }
}

pub struct Posting {
    pub account: String,
    pub amount: GbpValue,
}

impl fmt::Display for Posting {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}  {}", self.account, self.amount)
    }
}
