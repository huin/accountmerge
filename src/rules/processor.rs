use anyhow::Result;

use crate::internal::TransactionPostings;

pub trait TransactionProcessor {
    fn update_transactions(
        &self,
        trns: Vec<TransactionPostings>,
    ) -> Result<Vec<TransactionPostings>>;
}
