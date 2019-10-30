use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use failure::Error;
use ledger_parser::{Posting, Transaction};
use typed_generational_arena::{StandardArena, StandardIndex};

use crate::comment::Comment;
use crate::tags::{FINGERPRINT_TAG_PREFIX, UNKNOWN_ACCOUNT_TAG};

const BAD_POSTING_INDEX: &str = "internal error: used invalid PostingIndex";
const BAD_TRANSACTION_INDEX: &str = "internal error: used invalid TransactionIndex";

type PostingArena = StandardArena<PostingHolder>;
type PostingIndex = StandardIndex<PostingHolder>;
type TransactionArena = StandardArena<TransactionHolder>;
type TransactionIndex = StandardIndex<TransactionHolder>;

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(
        display = "input posting matches multiple destination postings with fingerprints: {}",
        fingerprints
    )]
    InputPostingMatchesMultiplePostings {
        fingerprints: MultipleMatchingFingerprints,
    },
}

#[derive(Debug)]
struct MultipleMatchingFingerprints(Vec<String>);
impl std::fmt::Display for MultipleMatchingFingerprints {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.write_str(&itertools::join(
            self.0.iter().map(|f| format!("{:?}", f)),
            ", ",
        ))
    }
}

pub struct Merger {
    post_arena: PostingArena,
    posts_by_date: HashMap<NaiveDate, Vec<PostingIndex>>,
    post_by_fingerprint: HashMap<String, PostingIndex>,
    trn_arena: TransactionArena,
    trns_by_date: HashMap<NaiveDate, Vec<TransactionIndex>>,
}

impl Merger {
    pub fn new() -> Self {
        Merger {
            post_arena: StandardArena::new(),
            posts_by_date: HashMap::new(),
            post_by_fingerprint: HashMap::new(),
            trn_arena: StandardArena::new(),
            trns_by_date: HashMap::new(),
        }
    }

    /// This merging algorithm is described in README.md under "Matching
    /// algorithm".
    pub fn merge(&mut self, src_trns: Vec<Transaction>) -> Result<(), Error> {
        // dest_posts' allocation is reused each iteration.
        let mut dest_posts = Vec::<Option<PostingIndex>>::new();

        for mut src_trn in src_trns.into_iter() {
            if src_trn.postings.is_empty() {
                continue;
            }

            let src_posts: Vec<InputPosting> = src_trn
                .postings
                .drain(..)
                .map(InputPosting::from_posting)
                .collect::<Result<Vec<InputPosting>, Error>>()?;
            // dst_posts contains indices to the corresponding existing posts
            // for those in src_posts, or None where no existing matching post
            // was found.
            dest_posts.clear();
            for src_post in &src_posts {
                dest_posts.push(self.find_matching_posting(src_post, src_trn.date)?);
            }

            // Determine default destination transaction.
            let default_dest_trn_idx = self.get_default_dest_trn(src_trn, &dest_posts);

            for (src_post, opt_dest_post_idx) in src_posts.into_iter().zip(&dest_posts) {
                match opt_dest_post_idx {
                    Some(dest_post_idx) => {
                        // Merge into existing posting.
                        let dest_post = self.get_post_mut(*dest_post_idx);
                        let opt_key = dest_post.merge_from_input_posting(src_post);
                        // The posting merged in might bring with it a new key
                        // to include in the index.
                        self.register_fingerprints(opt_key, *dest_post_idx);
                    }
                    None => {
                        // Create new posting.
                        let src_post_idx = self.add_posting(src_post, default_dest_trn_idx);
                        self.get_trn_mut(default_dest_trn_idx)
                            .postings
                            .push(src_post_idx);
                    }
                }
            }
        }

        Ok(())
    }

    // TODO: Replace expect calls with returned internal errors.

    fn get_post(&self, post_idx: PostingIndex) -> &PostingHolder {
        self.post_arena.get(post_idx).expect(BAD_POSTING_INDEX)
    }

    fn get_post_mut(&mut self, post_idx: PostingIndex) -> &mut PostingHolder {
        self.post_arena.get_mut(post_idx).expect(BAD_POSTING_INDEX)
    }

    fn get_trn(&self, trn_idx: TransactionIndex) -> &TransactionHolder {
        self.trn_arena.get(trn_idx).expect(BAD_TRANSACTION_INDEX)
    }

    fn get_trn_mut(&mut self, trn_idx: TransactionIndex) -> &mut TransactionHolder {
        self.trn_arena
            .get_mut(trn_idx)
            .expect(BAD_TRANSACTION_INDEX)
    }

    fn get_default_dest_trn(
        &mut self,
        src_trn: Transaction,
        dest_posts: &[Option<PostingIndex>],
    ) -> TransactionIndex {
        dest_posts
            .iter()
            .find_map(|opt_dest_post| {
                opt_dest_post.map(|dest_post| self.get_post(dest_post).parent_trn)
            })
            .unwrap_or_else(|| self.add_transaction(src_trn))
    }

    fn find_matching_posting(
        &self,
        post: &InputPosting,
        date: NaiveDate,
    ) -> Result<Option<PostingIndex>, Error> {
        let fp_idx = self.find_posting_by_fingerprints(post)?;
        if fp_idx.is_some() {
            return Ok(fp_idx);
        }

        // Look for a match based on internal values.
        let opt_post_idxs = self.posts_by_date.get(&date);
        let iter_posts_for_date = opt_post_idxs.iter().flat_map(|idxs| idxs.iter());
        for candidate_idx in iter_posts_for_date {
            let candidate_post = self.get_post(*candidate_idx);
            if candidate_post.matches(post) {
                return Ok(Some(*candidate_idx));
            }
        }

        Ok(None)
    }

    /// Look for match by existing fingerprint(s). Matches zero or one postings
    /// on success, multiple matches are an error.
    fn find_posting_by_fingerprints(
        &self,
        post: &InputPosting,
    ) -> Result<Option<PostingIndex>, Error> {
        let posts: HashSet<HashablePostingIndex> = post
            .fingerprints
            .iter()
            .filter_map(|fp| self.post_by_fingerprint.get(fp))
            .copied()
            .map(HashablePostingIndex)
            .collect();
        match posts.len() {
            n if n <= 1 => Ok(posts.iter().nth(0).map(|i| i.0)),
            _ => Err(MergeError::InputPostingMatchesMultiplePostings {
                fingerprints: MultipleMatchingFingerprints(post.fingerprints.clone()),
            }
            .into()),
        }
    }

    fn add_posting(
        &mut self,
        proto_posting: InputPosting,
        parent_trn: TransactionIndex,
    ) -> PostingIndex {
        let (posting, fingerprints) = PostingHolder::from_input_posting(proto_posting, parent_trn);
        let idx = self.post_arena.insert(posting);
        self.register_fingerprints(fingerprints, idx);

        self.posts_by_date
            .entry(self.get_trn(parent_trn).trn.date)
            .or_insert_with(Vec::new)
            .push(idx);
        idx
    }

    fn register_fingerprints(&mut self, fingerprints: Vec<String>, post_idx: PostingIndex) {
        for fp in fingerprints.into_iter() {
            self.post_by_fingerprint.insert(fp, post_idx);
        }
    }

    fn add_transaction(&mut self, trn: Transaction) -> TransactionIndex {
        let date = trn.date;
        let idx = self
            .trn_arena
            .insert(TransactionHolder::from_transaction(trn));
        self.trns_by_date
            .entry(date)
            .or_insert_with(Vec::new)
            .push(idx);
        idx
    }

    pub fn build(mut self) -> Vec<Transaction> {
        let mut dates: Vec<NaiveDate> = self.trns_by_date.keys().cloned().collect();
        dates.sort();
        // Avoid mutably borrowing self twice.
        let (mut trn_arena, mut trns_by_date) = {
            let mut trn_arena = TransactionArena::new();
            std::mem::swap(&mut trn_arena, &mut self.trn_arena);
            let mut trns_by_date = HashMap::new();
            std::mem::swap(&mut trns_by_date, &mut self.trns_by_date);
            (trn_arena, trns_by_date)
        };
        let mut out = Vec::<Transaction>::new();

        for date in &dates {
            if let Some(date_trn_indices) = trns_by_date.remove(date) {
                for trn_index in date_trn_indices {
                    let trn_holder = trn_arena.remove(trn_index).expect(BAD_TRANSACTION_INDEX);
                    let posts = trn_holder
                        .postings
                        .iter()
                        .map(|post_idx| {
                            self.post_arena
                                .remove(*post_idx)
                                .expect(BAD_POSTING_INDEX)
                                .into_posting()
                        })
                        .collect();
                    let trn = trn_holder.into_transaction(posts);
                    out.push(trn);
                }
            }
        }

        out
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

/// Contains a partially unpacked `Transaction`.
struct TransactionHolder {
    trn: Transaction,

    postings: Vec<PostingIndex>,
}

impl TransactionHolder {
    /// Moves trn into a new `TransactionHolder`, discarding any Postings
    /// inside.
    fn from_transaction(mut trn: Transaction) -> Self {
        trn.postings.clear();
        TransactionHolder {
            trn,
            postings: Vec::new(),
        }
    }

    fn into_transaction(mut self, postings: Vec<Posting>) -> Transaction {
        self.trn.postings = postings;
        self.trn
    }
}

struct InputPosting {
    fingerprints: Vec<String>,
    posting: Posting,
    comment: Comment,
}

impl InputPosting {
    fn from_posting(mut posting: Posting) -> Result<Self, Error> {
        let comment = Comment::from_opt_comment(posting.comment.as_ref().map(String::as_str));
        posting.comment = None;
        Ok(Self {
            fingerprints: fingerprints_from_comment(&comment),
            posting,
            comment,
        })
    }
}

/// Contains a partially unpacked `Posting`.
struct PostingHolder {
    parent_trn: TransactionIndex,
    posting: Posting,
    comment: Comment,
}

impl PostingHolder {
    fn from_input_posting(
        proto: InputPosting,
        parent_trn: TransactionIndex,
    ) -> (Self, Vec<String>) {
        (
            Self {
                parent_trn,
                posting: proto.posting,
                comment: proto.comment,
            },
            proto.fingerprints,
        )
    }

    fn into_posting(mut self) -> Posting {
        self.posting.comment = self.comment.into_opt_comment();
        self.posting
    }

    fn matches(&self, input: &InputPosting) -> bool {
        let b = &input.posting;
        self.posting.amount == b.amount
            && match (&self.posting.balance, &b.balance) {
                (Some(a_bal), Some(b_bal)) => a_bal == b_bal,
                _ => true,
            }
    }

    fn merge_from_input_posting(&mut self, mut src: InputPosting) -> Vec<String> {
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

#[derive(Eq, PartialEq)]
struct HashablePostingIndex(PostingIndex);
impl std::hash::Hash for HashablePostingIndex {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.arr_idx().hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::parse_transactions;

    #[test]
    fn stable_sorts_destination_by_date() {
        let mut merger = Merger::new();
        merger
            .merge(parse_transactions(
                r#"
            2000/02/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            2000/02/01 Lunch
                assets:checking  GBP -5.00
                expenses:dining  GBP 5.00
            "#,
            ))
            .unwrap();
        let result = merger.build();
        assert_transactions_eq!(
            &result,
            parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00
                    income:salary    GBP -100.00
                2000/02/01 Salary
                    assets:checking  GBP 100.00
                    income:salary    GBP -100.00
                2000/02/01 Lunch
                    assets:checking  GBP -5.00
                    expenses:dining  GBP 5.00
                "#
            ),
        );
    }

    #[test]
    fn dedupes_added() {
        let mut merger = Merger::new();

        merger
            .merge(parse_transactions(
                r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            "#,
            ))
            .unwrap();
        merger
            .merge(parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00
                    income:salary    GBP -100.00
                2000/01/02 Lunch
                    assets:checking  GBP -5.00
                    expenses:dining  GBP 5.00
                "#,
            ))
            .unwrap();
        let result = merger.build();
        assert_transactions_eq!(
            &result,
            parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00
                    income:salary    GBP -100.00
                2000/01/02 Lunch
                    assets:checking  GBP -5.00
                    expenses:dining  GBP 5.00
                "#
            ),
        );
    }

    #[test]
    fn fingerprint_matching() {
        let mut merger = Merger::new();

        merger
            .merge(parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00   ; :fp-1:fp-2:fp-3:
                "#,
            ))
            .unwrap();
        merger
            .merge(parse_transactions(
                // Different date to avoid soft-matching if fingerprint matching fails.
                r#"
                2000/01/02 Salary
                    assets:checking  GBP 100.00   ; :fp-1:fp-2:fp-4:
                "#,
            ))
            .unwrap();
        let result = merger.build();
        assert_transactions_eq!(
            &result,
            parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00  ; :fp-1:fp-2:fp-3:fp-4:
                "#
            ),
        );
    }

    // TODO: postings from a call to merge will currently match earlier postings
    // from the same call to merge. Should instead discount them as candidates.

    #[test]
    fn fingerprint_many_match_failure() {
        let mut merger = Merger::new();
        merger
            .merge(parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00   ; :fp-1:
                2000/01/02 Transfer to savings
                    assets:savings   GBP 100.00   ; :fp-2:
                "#,
            ))
            .unwrap();
        assert!(merger
            .merge(parse_transactions(
                // This posting has fingerprints matching two different postings
                // and should cause an error when atttempting to merge.
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00   ; :fp-1:fp-2:
                "#,
            ))
            .is_err());

        // The result should be the same as before attempting to merge.
        let result = merger.build();
        assert_transactions_eq!(
            &result,
            parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00   ; :fp-1:
                2000/01/02 Transfer to savings
                    assets:savings   GBP 100.00   ; :fp-2:
                "#
            ),
        );
    }

    #[test]
    fn balances_added_to_existing() {
        let mut merger = Merger::new();

        merger
            .merge(parse_transactions(
                r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            "#,
            ))
            .unwrap();
        merger
            .merge(parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00  =GBP 1234.00
                    income:salary    GBP -100.00
                "#,
            ))
            .unwrap();
        let result = merger.build();
        assert_transactions_eq!(
            &result,
            parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00  = GBP 1234.00
                    income:salary    GBP -100.00
                "#
            ),
        );
    }

    #[test]
    fn does_not_overwrite_some_fields() {
        let mut merger = Merger::new();

        merger
            .merge(parse_transactions(
                r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            "#,
            ))
            .unwrap();
        merger
            .merge(parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00  =GBP 1234.00
                    income:salary    GBP -100.00
                "#,
            ))
            .unwrap();
        let result = merger.build();
        assert_transactions_eq!(
            &result,
            parse_transactions(
                r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00  = GBP 1234.00
                    income:salary    GBP -100.00
                "#
            ),
        );
    }

    fn parse_posting(p: &str) -> Posting {
        let t = "2000/01/01 Dummy Transaction\n  ".to_string() + p + "\n";
        let mut trn = ledger_parser::parse(&t).unwrap();
        trn.transactions.remove(0).postings.remove(0)
    }

    #[test]
    fn test_merge_from_posting() {
        let dummy_idx = StandardIndex::from_idx_first_gen(0);
        let parse_merge_from = |dest: &str, src: &str| {
            let dest_posting = InputPosting::from_posting(parse_posting(dest)).unwrap();
            let src_posting = InputPosting::from_posting(parse_posting(src)).unwrap();
            let (mut dest_holder, _) = PostingHolder::from_input_posting(dest_posting, dummy_idx);
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
        let dummy_idx = StandardIndex::from_idx_first_gen(0);
        let parse_match = |dest: &str, src: &str| {
            let dest_posting = InputPosting::from_posting(parse_posting(dest)).unwrap();
            let src_posting = InputPosting::from_posting(parse_posting(src)).unwrap();
            let (dest_holder, _) = PostingHolder::from_input_posting(dest_posting, dummy_idx);
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
            parse_match("foo  GBP 10.00", "bar  GBP 10.00"),
            true,
            "Differing account.",
        );
    }
}
