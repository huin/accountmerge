use std::collections::HashMap;

use chrono::NaiveDate;
use failure::Error;
use ledger_parser::Posting;
use typed_generational_arena::{StandardArena, StandardIndex};

use crate::comment::Comment;
use crate::merge::transaction;
use crate::tags::{FINGERPRINT_TAG_PREFIX, UNKNOWN_ACCOUNT_TAG};

const BAD_POSTING_INDEX: &str = "internal error: used invalid posting::Index";

pub type Arena = StandardArena<Holder>;
pub type Index = StandardIndex<Holder>;

pub struct IndexedPostings {
    post_arena: Arena,
    posts_by_date: HashMap<NaiveDate, Vec<Index>>,
    post_by_fingerprint: HashMap<String, Index>,
}

impl IndexedPostings {
    pub fn new() -> Self {
        Self {
            post_arena: Arena::new(),
            posts_by_date: HashMap::new(),
            post_by_fingerprint: HashMap::new(),
        }
    }

    pub fn into_consume(self) -> ConsumePostings {
        ConsumePostings(self.post_arena)
    }

    /// Adds a new posting, updating the fingerprint index.
    pub fn add(&mut self, input_posting: Input, parent_trn: transaction::Index) -> Index {
        let (posting, trn_date, fingerprints) = Holder::from_input(input_posting, parent_trn);
        let idx = self.post_arena.insert(posting);
        self.register_fingerprints(fingerprints, idx);

        self.posts_by_date
            .entry(trn_date)
            .or_insert_with(Vec::new)
            .push(idx);
        idx
    }

    pub fn fingerprint_to_index(&self, fingerprint: &str) -> Option<Index> {
        self.post_by_fingerprint.get(fingerprint).copied()
    }

    // TODO: Replace expect calls with returned internal errors.

    pub fn get(&self, post_idx: Index) -> &Holder {
        self.post_arena.get(post_idx).expect(BAD_POSTING_INDEX)
    }

    fn get_mut(&mut self, post_idx: Index) -> &mut Holder {
        self.post_arena.get_mut(post_idx).expect(BAD_POSTING_INDEX)
    }

    pub fn date_to_indices<'a>(&'a self, date: NaiveDate) -> impl Iterator<Item = Index> + 'a {
        let opt_vec = self.posts_by_date.get(&date);
        opt_vec.into_iter().flat_map(|vec| vec.iter()).copied()
    }

    /// Updates an existing posting, updating the fingerprint index.
    pub fn merge_into(&mut self, existing_post_idx: Index, input_posting: Input) {
        let dest_post = self.get_mut(existing_post_idx);
        let post_fingerprints = dest_post.merge_from_input_posting(input_posting);
        self.register_fingerprints(post_fingerprints, existing_post_idx);
    }

    /// Adds to posting fingerprints index.
    fn register_fingerprints(&mut self, fingerprints: Vec<String>, post_idx: Index) {
        for fp in fingerprints.into_iter() {
            self.post_by_fingerprint.insert(fp, post_idx);
        }
    }
}

pub struct ConsumePostings(Arena);

impl ConsumePostings {
    pub fn take(&mut self, post_idx: Index) -> Posting {
        self.0
            .remove(post_idx)
            .expect(BAD_POSTING_INDEX)
            .into_posting()
    }
}

pub struct Input {
    fingerprints: Vec<String>,
    trn_date: NaiveDate,
    posting: Posting,
    comment: Comment,
}

impl Input {
    pub fn from_posting(mut posting: Posting, trn_date: NaiveDate) -> Result<Self, Error> {
        let comment = Comment::from_opt_comment(posting.comment.as_ref().map(String::as_str));
        posting.comment = None;
        Ok(Self {
            fingerprints: fingerprints_from_comment(&comment),
            trn_date,
            posting,
            comment,
        })
    }

    pub fn iter_fingerprints<'a>(&'a self) -> impl Iterator<Item = &str> + 'a {
        self.fingerprints.iter().map(String::as_str)
    }
}

/// Contains a partially unpacked `Posting`.
pub struct Holder {
    parent_trn: transaction::Index,
    posting: Posting,
    comment: Comment,
}

impl Holder {
    fn from_input(proto: Input, parent_trn: transaction::Index) -> (Self, NaiveDate, Vec<String>) {
        (
            Self {
                parent_trn,
                posting: proto.posting,
                comment: proto.comment,
            },
            proto.trn_date,
            proto.fingerprints,
        )
    }

    fn into_posting(mut self) -> Posting {
        self.posting.comment = self.comment.into_opt_comment();
        self.posting
    }

    pub fn get_parent_trn(&self) -> transaction::Index {
        self.parent_trn
    }

    pub fn matches(&self, input: &Input) -> bool {
        let a = &self.posting;
        let b = &input.posting;

        let accounts_match = if !self.comment.tags.contains(UNKNOWN_ACCOUNT_TAG)
            && !input.comment.tags.contains(UNKNOWN_ACCOUNT_TAG)
        {
            a.account == b.account
        } else {
            true
        };

        let amounts_match = a.amount == b.amount;

        let balances_match = match (&a.balance, &b.balance) {
            (Some(a_bal), Some(b_bal)) => a_bal == b_bal,
            _ => true,
        };

        accounts_match && amounts_match && balances_match
    }

    fn merge_from_input_posting(&mut self, mut src: Input) -> Vec<String> {
        // TODO: Merge/update status.
        if self.posting.balance.is_none() {
            self.posting.balance = src.posting.balance.clone()
        }
        if self.comment.tags.contains(UNKNOWN_ACCOUNT_TAG)
            && !src.comment.tags.contains(UNKNOWN_ACCOUNT_TAG)
        {
            self.comment.tags.remove(UNKNOWN_ACCOUNT_TAG);
            self.posting.account = src.posting.account;
        }
        src.comment.tags.remove(UNKNOWN_ACCOUNT_TAG);

        self.comment.merge_from(src.comment);
        src.fingerprints
    }
}

/// Extracts copies of the fingerprint tag(s) from `comment`.
fn fingerprints_from_comment(comment: &Comment) -> Vec<String> {
    comment
        .tags
        .iter()
        .filter(|t| t.starts_with(FINGERPRINT_TAG_PREFIX))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::parse_posting;

    #[test]
    fn test_merge_from_posting() {
        let dummy_date = NaiveDate::from_ymd(2000, 1, 1);
        let dummy_idx = StandardIndex::from_idx_first_gen(0);
        let parse_merge_from = |dest: &str, src: &str| {
            let dest_posting = Input::from_posting(parse_posting(dest), dummy_date).unwrap();
            let src_posting = Input::from_posting(parse_posting(src), dummy_date).unwrap();
            let (mut dest_holder, _, _) = Holder::from_input(dest_posting, dummy_idx);
            dest_holder.merge_from_input_posting(src_posting);
            dest_holder.into_posting()
        };
        assert_eq!(
            parse_merge_from("foo  GBP 10.00", "foo  GBP 10.00 =GBP 90.00"),
            parse_posting("foo  GBP 10.00 =GBP 90.00"),
            "updates None balance",
        );
        assert_eq!(
            parse_merge_from("foo  GBP 10.00 =GBP 50.00", "foo  GBP 10.00 =GBP 90.00"),
            parse_posting("foo  GBP 10.00 =GBP 50.00"),
            "does not update existing balance",
        );
        assert_eq!(
            parse_merge_from(
                "foo  GBP 10.00 =GBP 50.00 ; key: old-value",
                "foo  GBP 10.00 =GBP 90.00 ; key: new-value"
            ),
            parse_posting("foo  GBP 10.00 =GBP 50.00 ; key: new-value"),
            "merges comments",
        );
        assert_eq!(
            parse_merge_from("foo  GBP 10.00", "bar  GBP 10.00 ; :unknown-account:"),
            parse_posting("foo  GBP 10.00"),
            "Does not update from unknown account.",
        );
        assert_eq!(
            parse_merge_from(
                "foo  GBP 10.00 ; :unknown-account:",
                "bar  GBP 10.00 ; :unknown-account:"
            ),
            parse_posting("foo  GBP 10.00 ; :unknown-account:"),
            "Does not update unknown account from unknown account.",
        );
        assert_eq!(
            parse_merge_from("foo  GBP 10.00 ; :unknown-account:", "bar  GBP 10.00"),
            parse_posting("bar  GBP 10.00"),
            "Updates unknown account and removes unknown-account tag.",
        );
    }

    #[test]
    fn test_match_posting() {
        let dummy_date = NaiveDate::from_ymd(2000, 1, 1);
        let dummy_idx = StandardIndex::from_idx_first_gen(0);
        let parse_match = |dest: &str, src: &str| {
            let dest_posting = Input::from_posting(parse_posting(dest), dummy_date).unwrap();
            let src_posting = Input::from_posting(parse_posting(src), dummy_date).unwrap();
            let (dest_holder, _, _) = Holder::from_input(dest_posting, dummy_idx);
            dest_holder.matches(&src_posting)
        };
        assert_eq!(
            parse_match("foo  GBP 10.00 =GBP 90.00", "foo  GBP 10.00 =GBP 90.00"),
            true,
            "Have same balances.",
        );
        assert_eq!(
            parse_match("foo  GBP 10.00 =GBP 23.00", "foo  GBP 10.00 =GBP 90.00"),
            false,
            "Have differing balances."
        );
        assert_eq!(
            parse_match("foo  GBP 10.00", "foo  GBP 10.00 =GBP 90.00"),
            true,
            "Only source balance.",
        );
        assert_eq!(
            parse_match("foo  GBP 10.00 =GBP 90.00", "foo  GBP 10.00"),
            true,
            "Only dest balance.",
        );
        assert_eq!(
            parse_match("foo  GBP 10.00", "foo  GBP 10.00"),
            true,
            "Same amount.",
        );
        assert_eq!(
            parse_match("foo  GBP 23.00", "foo  GBP 10.00"),
            false,
            "Differing amount.",
        );
        assert_eq!(
            parse_match("foo  GBP 10.00", "bar  GBP 10.00  ; :unknown-account:"),
            true,
            "Differing unknown source account.",
        );
        assert_eq!(
            parse_match("foo  GBP 10.00  ; :unknown-account:", "bar  GBP 10.00"),
            true,
            "Differing unknown dest account.",
        );
        assert_eq!(
            parse_match(
                "foo  GBP 10.00  ; :unknown-account:",
                "bar  GBP 10.00  ; :unknown-account:"
            ),
            true,
            "Differing unknown accounts match.",
        );
        assert_eq!(
            parse_match("foo  GBP 10.00", "bar  GBP 10.00"),
            false,
            "Differing known accounts do not match.",
        );
    }
}
