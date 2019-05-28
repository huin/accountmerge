use chrono::NaiveDate;

use crate::money::GbpValue;

#[derive(Clone, Copy, Debug)]
pub enum Paid {
    In(GbpValue),
    Out(GbpValue),
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
