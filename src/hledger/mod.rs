use std::fmt;

use chrono::{Datelike, NaiveDate};

use crate::money::GbpValue;

pub mod parser;

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

pub struct TransactionBuilder {
    trn: Transaction,
}

impl TransactionBuilder {
    pub fn new<S: Into<String>>(date: NaiveDate, description: S) -> Self {
        TransactionBuilder {
            trn: Transaction {
                date,
                description: description.into(),
                postings: vec![],
            },
        }
    }

    pub fn posting<S: Into<String>>(
        mut self,
        account: S,
        amount: GbpValue,
        balance: Option<GbpValue>,
    ) -> Self {
        self.trn.postings.push(Posting {
            account: account.into(),
            amount,
            balance,
        });
        self
    }

    pub fn build(self) -> Transaction {
        self.trn
    }
}

pub struct Posting {
    pub account: String,
    pub amount: GbpValue,
    pub balance: Option<GbpValue>,
}

impl fmt::Display for Posting {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}  {}", self.account, self.amount)?;
        if let Some(balance) = &self.balance {
            write!(f, "  ={}", balance)?;
        }
        Ok(())
    }
}
