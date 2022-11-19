use anyhow::Result;
use ledger_parser::Transaction;

pub struct Import {
    /// User namespace for fingerprints.
    pub user_fp_namespace: String,
    /// Imported transactions.
    pub transactions: Vec<Transaction>,
}

pub trait TransactionImporter {
    fn get_transactions(&self) -> Result<Import>;
}
