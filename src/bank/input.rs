use chrono::NaiveDate;
use ledger_parser::Amount;

#[derive(Debug)]
pub struct InputTransaction {
    pub bank: String,
    pub account_name: String,
    pub date: NaiveDate,
    pub type_: String,
    pub description: String,
    pub paid: Amount,
    pub balance: Amount,
}
