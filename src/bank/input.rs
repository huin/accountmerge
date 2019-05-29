use std::convert::TryInto;

use chrono::NaiveDate;

use crate::money::{GbpValue, UnsignedGbpValue, MoneyError};

#[derive(Clone, Copy, Debug)]
pub enum Paid {
    In(UnsignedGbpValue),
    Out(UnsignedGbpValue),
}

impl Paid {
    pub fn src_acct_amt(self) -> Result<GbpValue, MoneyError> {
        match self {
            Paid::In(v) => v.try_into(),
            Paid::Out(v) => v.try_into().map(|v: GbpValue| -v),
        }
    }

    pub fn dest_acct_amt(self) -> Result<GbpValue, MoneyError> {
        match self {
            Paid::In(v) => v.try_into().map(|v: GbpValue| -v),
            Paid::Out(v) => v.try_into(),
        }
    }
}

#[derive(Debug)]
pub struct InputTransaction {
    pub src_bank: String,
    pub src_acct: String,
    pub date: NaiveDate,
    pub type_: String,
    pub description: String,
    pub paid: Paid,
    pub balance: GbpValue,
}
