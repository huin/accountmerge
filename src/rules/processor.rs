use anyhow::Result;

use crate::internal::TransactionPostings;

pub trait TransactionProcessorFactory {
    fn make_processor(&self) -> Result<Box<dyn TransactionProcessor>>;
}

pub trait TransactionProcessor {
    fn update_transactions(
        &self,
        trns: Vec<TransactionPostings>,
    ) -> Result<Vec<TransactionPostings>>;
}
