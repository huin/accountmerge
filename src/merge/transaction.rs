use ledger_parser::{Posting, Transaction};
use typed_generational_arena::{StandardArena, StandardIndex};

use crate::merge::posting;

pub type Arena = StandardArena<Holder>;
pub type Index = StandardIndex<Holder>;

/// Contains a partially unpacked `Transaction`.
pub struct Holder {
    pub trn: Transaction,

    pub postings: Vec<posting::Index>,
}

impl Holder {
    /// Moves trn into a new `Holder`, moving out any Postings
    /// inside.
    pub fn from_transaction(mut trn: Transaction) -> (Self, Vec<Posting>) {
        let mut posts: Vec<Posting> = Vec::new();
        std::mem::swap(&mut posts, &mut trn.postings);
        (
            Holder {
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
