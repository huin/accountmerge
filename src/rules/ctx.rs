use crate::internal::{PostingInternal, TransactionInternal};

pub struct PostingContext<'a> {
    pub trn: &'a mut TransactionInternal,
    pub post: &'a mut PostingInternal,
}
