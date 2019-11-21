use std::collections::HashMap;

use chrono::NaiveDate;
use failure::Error;
use ledger_parser::Posting;
use typed_generational_arena::{StandardArena, StandardIndex};

use crate::comment::Comment;
use crate::merge::matchset::MatchSet;
use crate::merge::transaction;
use crate::merge::MergeError;
use crate::tags::{CANDIDATE_FP_TAG_PREFIX, FINGERPRINT_TAG_PREFIX, UNKNOWN_ACCOUNT_TAG};

const BAD_POSTING_INDEX: &str = "internal error: used invalid posting::Index";

pub type Arena = StandardArena<Holder>;
pub type Index = StandardIndex<Holder>;

/// A newtype to allow using `Index` in a `HashSet` or `HashMap` key.
#[derive(Eq)]
pub struct IndexHashable(pub Index);
impl PartialEq for IndexHashable {
    fn eq(&self, other: &Self) -> bool {
        self.0.arr_idx() == other.0.arr_idx() && self.0.gen() == other.0.gen()
    }
}
impl std::hash::Hash for IndexHashable {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.arr_idx().hash(state);
    }
}

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
    pub fn add(&mut self, input: Input, parent_trn: transaction::Index) -> Result<Index, Error> {
        let fingerprints: Vec<String> = fingerprints_from_comment(&input.comment)
            .map(str::to_string)
            .collect();
        let (holder, trn_date) = Holder::from_input(input, parent_trn);
        let idx = self.post_arena.insert(holder);
        self.register_fingerprints(fingerprints.into_iter(), idx)?;

        self.posts_by_date
            .entry(trn_date)
            .or_insert_with(Vec::new)
            .push(idx);
        Ok(idx)
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
    pub fn merge_into(
        &mut self,
        existing_post_idx: Index,
        input_posting: Input,
    ) -> Result<(), Error> {
        self.register_fingerprints(
            fingerprints_from_comment(&input_posting.comment).map(str::to_string),
            existing_post_idx,
        )?;
        let dest_post = self.get_mut(existing_post_idx);
        dest_post.merge_from_input_posting(input_posting);
        Ok(())
    }

    /// Adds fingerprints to posting fingerprints index.
    fn register_fingerprints(
        &mut self,
        fingerprints: impl Iterator<Item = String>,
        post_idx: Index,
    ) -> Result<(), Error> {
        for fp in fingerprints {
            use std::collections::hash_map::Entry::*;
            match self.post_by_fingerprint.entry(fp.to_string()) {
                Occupied(e) => {
                    if e.get() != &post_idx {
                        let reason = format!(
                            "multiple posts claiming fingerprint {:?} added or merged",
                            fp
                        );
                        return Err(MergeError::Internal { reason }.into());
                    }
                }
                Vacant(e) => {
                    e.insert(post_idx);
                }
            }
        }
        Ok(())
    }

    pub fn find_matching_postings(&self, post: &Input) -> Match {
        use MatchSet::*;
        match self.find_posting_by_fingerprints(post) {
            One(idx) => Match::Fingerprint(MatchedIndices::One(idx)),
            Many(idxs) => Match::Fingerprint(MatchedIndices::Many(idxs.into_iter().collect())),
            Zero => {
                // Look for a match based on internal values.
                let soft_idxs: MatchSet<Index> = self
                    .date_to_indices(post.trn_date)
                    .filter(|idx| {
                        let candidate = self.get(*idx);
                        candidate.matches(post)
                    })
                    .collect();

                match soft_idxs {
                    One(idx) => Match::Soft(MatchedIndices::One(idx)),
                    Many(idxs) => Match::Soft(MatchedIndices::Many(idxs.into_iter().collect())),
                    Zero => Match::Zero,
                }
            }
        }
    }

    /// Look for match by existing fingerprint(s). Matches zero or one postings
    /// on success, multiple matches are an error.
    fn find_posting_by_fingerprints(&self, post: &Input) -> MatchSet<Index> {
        post.iter_fingerprints()
            .filter_map(|fp| self.fingerprint_to_index(fp))
            .collect()
    }
}

pub enum Match {
    Fingerprint(MatchedIndices),
    Soft(MatchedIndices),
    Zero,
}

pub enum MatchedIndices {
    One(Index),
    Many(Vec<Index>),
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
    trn_date: NaiveDate,
    posting: Posting,
    comment: Comment,
}

impl Input {
    pub fn from_posting(mut posting: Posting, trn_date: NaiveDate) -> Result<Self, Error> {
        let comment = Comment::from_opt_comment(posting.comment.as_ref().map(String::as_str));
        posting.comment = None;

        // Error if any src_post has a candidate tag on it. The user
        // should have removed it.
        if comment
            .tags
            .iter()
            .any(|tag| tag.starts_with(CANDIDATE_FP_TAG_PREFIX))
        {
            return Err(MergeError::Input {
                reason: format!("posting \"{}\" has a candidate tag", posting),
            }
            .into());
        }

        // Ensure that there is at least one fingerprint to serve as the
        // primary. Having at least one fingerprint is required by the merging
        // process. I.e `primary_fingerprint` may panic if we don't check this.
        if !comment
            .tags
            .iter()
            .any(|tag| tag.starts_with(FINGERPRINT_TAG_PREFIX))
        {
            return Err(MergeError::Input {
                reason: format!("posting \"{}\" does not have a fingerprint tag", posting),
            }
            .into());
        }

        Ok(Self {
            trn_date,
            posting,
            comment,
        })
    }

    pub fn get_posting(&self) -> &Posting {
        &self.posting
    }

    pub fn into_posting(self) -> Posting {
        let mut posting = self.posting;
        posting.comment = self.comment.into_opt_comment();
        posting
    }

    pub fn add_tag(&mut self, tag: String) {
        self.comment.tags.insert(tag);
    }

    pub fn iter_fingerprints<'a>(&'a self) -> impl Iterator<Item = &str> + 'a {
        fingerprints_from_comment(&self.comment)
    }
}

/// Contains a partially unpacked `Posting`.
pub struct Holder {
    parent_trn: transaction::Index,
    posting: Posting,
    pub comment: Comment,
}

impl Holder {
    fn from_input(proto: Input, parent_trn: transaction::Index) -> (Self, NaiveDate) {
        (
            Self {
                parent_trn,
                posting: proto.posting,
                comment: proto.comment,
            },
            proto.trn_date,
        )
    }

    fn into_posting(mut self) -> Posting {
        self.posting.comment = self.comment.into_opt_comment();
        self.posting
    }

    pub fn get_posting(&self) -> &Posting {
        &self.posting
    }

    pub fn get_parent_trn(&self) -> transaction::Index {
        self.parent_trn
    }

    pub fn primary_fingerprint(&self) -> &str {
        primary_fingerprint(&self.comment)
    }

    fn matches(&self, input: &Input) -> bool {
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

    fn merge_from_input_posting(&mut self, mut src: Input) {
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
    }
}

fn primary_fingerprint(comment: &Comment) -> &str {
    fingerprints_from_comment(comment)
        .nth(0)
        .expect("must always have a fingerprint tag")
}

/// Extracts the fingerprint tag(s) from `comment`.
fn fingerprints_from_comment(comment: &Comment) -> impl Iterator<Item = &str> {
    comment
        .tags
        .iter()
        .map(String::as_str)
        .filter(|t| t.starts_with(FINGERPRINT_TAG_PREFIX))
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;
    use crate::testutil::{normalize_comment, parse_posting};

    #[test_case(
       "foo  GBP 10.00  ; :fp-1:",
       "foo  GBP 10.00 =GBP 90.00  ; :fp-1:",
       "foo  GBP 10.00 =GBP 90.00  ; :fp-1:";
       "updates None balance"
    )]
    #[test_case(
       "foo  GBP 10.00 =GBP 50.00  ; :fp-1:",
       "foo  GBP 10.00 =GBP 90.00  ; :fp-1:",
       "foo  GBP 10.00 =GBP 50.00  ; :fp-1:";
       "does not update existing balance"
    )]
    #[test_case(
       "foo  GBP 10.00 =GBP 50.00 ; :fp-1:\n  ; key: old-value",
       "foo  GBP 10.00 =GBP 90.00 ; :fp-2:\n  ; key: new-value",
       "foo  GBP 10.00 =GBP 50.00 ; :fp-1:fp-2:\n  ; key: new-value";
       "merges comments"
    )]
    #[test_case(
       "foo  GBP 10.00 ; :fp-1:",
       "bar  GBP 10.00 ; :fp-1:unknown-account:",
       "foo  GBP 10.00 ; :fp-1:";
       "Does not update from unknown account."
    )]
    #[test_case(
       "foo  GBP 10.00 ; :fp-1:unknown-account:",
       "bar  GBP 10.00 ; :fp-1:unknown-account:",
       "foo  GBP 10.00 ; :fp-1:unknown-account:";
       "Does not update unknown account from unknown account."
    )]
    #[test_case(
       "foo  GBP 10.00 ; :fp-1:unknown-account:",
       "bar  GBP 10.00 ; :fp-1:",
       "bar  GBP 10.00 ; :fp-1:";
       "Updates unknown account and removes unknown-account tag."
    )]
    fn input_from_posting(dest: &str, src: &str, want: &str) {
        let dummy_date = NaiveDate::from_ymd(2000, 1, 1);
        let dummy_idx = StandardIndex::from_idx_first_gen(0);

        let dest_posting = Input::from_posting(parse_posting(dest), dummy_date).unwrap();
        let src_posting = Input::from_posting(parse_posting(src), dummy_date).unwrap();
        let (mut dest_holder, _) = Holder::from_input(dest_posting, dummy_idx);
        dest_holder.merge_from_input_posting(src_posting);
        let mut result = dest_holder.into_posting();
        normalize_comment(&mut result.comment);

        assert_eq!(result, parse_posting(want));
    }

    #[test_case(
        "foo  GBP 10.00 =GBP 90.00  ; :fp-1:",
        "foo  GBP 10.00 =GBP 90.00  ; :fp-1:",
        true;
        "have_same_balances"
    )]
    #[test_case(
        "foo  GBP 10.00 =GBP 23.00  ; :fp-1:",
        "foo  GBP 10.00 =GBP 90.00  ; :fp-1:",
        false;
        "have_differing_balances"
    )]
    #[test_case(
        "foo  GBP 10.00  ; :fp-1:",
        "foo  GBP 10.00 =GBP 90.00  ; :fp-1:",
        true;
        "only_source_balance"
    )]
    #[test_case(
        "foo  GBP 10.00 =GBP 90.00  ; :fp-1:",
        "foo  GBP 10.00  ; :fp-1:",
        true;
        "only_dest_balance"
    )]
    #[test_case(
        "foo  GBP 10.00  ; :fp-1:",
        "foo  GBP 10.00  ; :fp-1:",
        true;
        "same_amount"
    )]
    #[test_case(
        "foo  GBP 23.00  ; :fp-1:",
        "foo  GBP 10.00  ; :fp-1:",
        false;
        "differing_amount"
    )]
    #[test_case(
        "foo  GBP 10.00  ; :fp-1:",
        "bar  GBP 10.00  ; :fp-1:unknown-account:",
        true;
        "differing_unknown_source_account"
    )]
    #[test_case(
        "foo  GBP 10.00  ; :fp-1:unknown-account:",
        "bar  GBP 10.00  ; :fp-1:",
        true;
        "differing_unknown_dest_account"
    )]
    #[test_case(
        "foo  GBP 10.00  ; :fp-1:unknown-account:",
        "bar  GBP 10.00  ; :fp-1:unknown-account:",
        true;
        "differing_unknown_accounts_match"
    )]
    #[test_case(
        "foo  GBP 10.00  ; :fp-1:",
        "bar  GBP 10.00  ; :fp-1:",
        false;
        "differing_known_accounts_do_not_match"
    )]
    fn holding_matches(dest: &str, src: &str, want: bool) {
        let dummy_date = NaiveDate::from_ymd(2000, 1, 1);
        let dummy_idx = StandardIndex::from_idx_first_gen(0);

        let dest_posting = Input::from_posting(parse_posting(dest), dummy_date).unwrap();
        let src_posting = Input::from_posting(parse_posting(src), dummy_date).unwrap();
        let (dest_holder, _) = Holder::from_input(dest_posting, dummy_idx);
        let got = dest_holder.matches(&src_posting);

        assert_eq!(got, want);
    }
}
