use failure::Error;
use ledger_parser::Transaction;

pub trait TransactionImporter {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error>;
}
