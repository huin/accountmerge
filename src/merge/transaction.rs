use ledger_parser::{Posting, Transaction};
use typed_generational_arena::{StandardArena, StandardIndex};

use crate::merge::posting;

pub type TransactionArena = StandardArena<TransactionHolder>;
pub type TransactionIndex = StandardIndex<TransactionHolder>;

/// Contains a partially unpacked `Transaction`.
pub struct TransactionHolder {
    pub trn: Transaction,

    pub postings: Vec<posting::PostingIndex>,
}

impl TransactionHolder {
    /// Moves trn into a new `TransactionHolder`, moving out any Postings
    /// inside.
    pub fn from_transaction(mut trn: Transaction) -> (Self, Vec<Posting>) {
        let mut posts: Vec<Posting> = Vec::new();
        std::mem::swap(&mut posts, &mut trn.postings);
        (
            TransactionHolder {
                trn,
                postings: Vec::new(),
            },
            posts,
        )
    }

    pub fn into_transaction(mut self, postings: Vec<Posting>) -> Transaction {
        self.trn.postings = postings;
        self.trn
    }
}
