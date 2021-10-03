use std::collections::HashMap;

use chrono::NaiveDate;
use typed_generational_arena::{StandardArena, StandardIndex};

use crate::internal::{PostingInternal, TransactionInternal, TransactionPostings};
use crate::merge::posting;

const BAD_TRANSACTION_INDEX: &str = "internal error: used invalid transaction::Index";

pub type Arena = StandardArena<Holder>;
pub type Index = StandardIndex<Holder>;

pub struct IndexedTransactions {
    trn_arena: Arena,
    trns_by_date: HashMap<NaiveDate, Vec<Index>>,
}

impl IndexedTransactions {
    pub fn new() -> Self {
        Self {
            trn_arena: StandardArena::new(),
            trns_by_date: HashMap::new(),
        }
    }

    /// Iterates over the transactions in date order, preserving insertion
    /// order.
    pub fn into_iter(self) -> impl Iterator<Item = Holder> {
        let mut trn_arena = self.trn_arena;
        let mut date_trns: Vec<(NaiveDate, Vec<Holder>)> = self
            .trns_by_date
            .into_iter()
            .map(|(date, indices)| {
                let holders: Vec<Holder> = indices
                    .into_iter()
                    .map(|index| trn_arena.remove(index).expect(BAD_TRANSACTION_INDEX))
                    .collect();
                (date, holders)
            })
            .collect();
        // Sort by dates (first item in tuple).
        date_trns.sort_by(|a, b| a.0.cmp(&b.0));

        date_trns
            .into_iter()
            .flat_map(|(_date, holders)| holders.into_iter())
    }

    // TODO: Replace expect calls with returned internal errors.

    pub fn get(&self, trn_idx: Index) -> &Holder {
        self.trn_arena.get(trn_idx).expect(BAD_TRANSACTION_INDEX)
    }

    fn get_mut(&mut self, trn_idx: Index) -> &mut Holder {
        self.trn_arena
            .get_mut(trn_idx)
            .expect(BAD_TRANSACTION_INDEX)
    }

    pub fn add(&mut self, trn: Holder) -> Index {
        let date = trn.trn.raw.date;
        let idx = self.trn_arena.insert(trn);
        self.trns_by_date
            .entry(date)
            .or_insert_with(Vec::new)
            .push(idx);
        idx
    }

    pub fn add_post_to_trn(&mut self, trn_idx: Index, post_idx: posting::Index) {
        let dest_trn = self.get_mut(trn_idx);
        dest_trn.postings.push(post_idx);
    }
}

/// Contains a partially unpacked `Transaction` with arena references to its
/// `Postings`.
pub struct Holder {
    pub trn: TransactionInternal,

    postings: Vec<posting::Index>,
}

impl Holder {
    /// Moves trn into a new `Holder`, moving out any Postings
    /// inside.
    pub fn from_transaction_internal(trn: TransactionInternal) -> Self {
        Holder {
            trn,
            postings: Vec::new(),
        }
    }

    pub fn into_transaction_postings(self, postings: Vec<PostingInternal>) -> TransactionPostings {
        TransactionPostings {
            trn: self.trn,
            posts: postings,
        }
    }

    pub fn iter_posting_indices(&'_ self) -> impl Iterator<Item = posting::Index> + '_ {
        self.postings.iter().copied()
    }
}
