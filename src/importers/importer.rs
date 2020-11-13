use anyhow::Result;
use ledger_parser::Transaction;

pub trait TransactionImporter {
    fn get_transactions(&self) -> Result<Vec<Transaction>>;
}
