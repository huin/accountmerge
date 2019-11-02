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
    #[fail(
        display = "multiple postings with same fingerprint ({:?}) found within a single input transaction set",
        fingerprint
    )]
    DuplicateFingerprint { fingerprint: String },
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
        let pending = self.make_pending(src_trns)?;
        self.apply_pending(pending);
        Ok(())
    }

    fn make_pending(&self, orig_trns: Vec<Transaction>) -> Result<PendingMerges, Error> {
        let mut pending = PendingMerges::new();

        // Set of fingerprints found in `pending.posts` so far.
        // This is used to check if duplicate fingerprints exist in the input.
        let mut fingerprints = HashSet::<String>::new();

        for orig_trn in orig_trns.into_iter() {
            let (src_trn, orig_posts) = TransactionHolder::from_transaction(orig_trn);

            if orig_posts.is_empty() {
                continue;
            }

            let src_posts_matched: Vec<(InputPosting, Option<PostingIndex>)> = orig_posts
                .into_iter()
                .map(|orig_post| {
                    let src_post = InputPosting::from_posting(orig_post)?;
                    let dest_post: Option<PostingIndex> =
                        self.find_matching_posting(&src_post, src_trn.trn.date)?;
                    Ok((src_post, dest_post))
                })
                .collect::<Result<Vec<(InputPosting, Option<PostingIndex>)>, Error>>()?;

            for (src_post, _) in &src_posts_matched {
                for fp in src_post.fingerprints.iter().cloned() {
                    if fingerprints.contains(&fp) {
                        return Err(MergeError::DuplicateFingerprint { fingerprint: fp }.into());
                    }
                    fingerprints.insert(fp);
                }
            }

            // Determine default destination transaction.
            let default_dest_trn: DestinationTransaction = src_posts_matched
                .iter()
                // TODO: Consider checking that only one destination transaction
                // matches.
                .find_map(|(_, opt_dest_post)| {
                    opt_dest_post
                        .map(|dest_post| self.get_post(dest_post).parent_trn)
                        .map(DestinationTransaction::Existing)
                })
                .unwrap_or_else(|| DestinationTransaction::New(pending.new_trns.insert(src_trn)));

            pending.posts.extend(
                src_posts_matched
                    .into_iter()
                    .map(|(src_post, opt_dest_post)| {
                        let destination = match opt_dest_post {
                            // Merge into existing posting.
                            Some(dest_post) => PostingDestination::MergeIntoExisting(dest_post),
                            // Create new posting, added to the default destination
                            // transaction.
                            None => PostingDestination::AddToTransaction(default_dest_trn),
                        };

                        PendingPosting {
                            post: src_post,
                            destination,
                        }
                    }),
            );
        }
        Ok(pending)
    }

    fn apply_pending(&mut self, mut pending: PendingMerges) {
        // Maps from index in pending.new_trns to index in self.trn_arena.
        let new_trn_idxs: HashMap<HashableTransactionIndex, HashableTransactionIndex> = pending
            .new_trns
            .drain()
            .map(|(pending_trn_idx, trn)| {
                (
                    HashableTransactionIndex(pending_trn_idx),
                    HashableTransactionIndex(self.add_transaction_holder(trn)),
                )
            })
            .collect();

        for post in pending.posts.drain(..) {
            use DestinationTransaction::*;
            use PostingDestination::*;

            match post.destination {
                MergeIntoExisting(post_idx) => {
                    let dest_post = self.get_post_mut(post_idx);
                    let post_fingerprints = dest_post.merge_from_input_posting(post.post);
                    self.register_fingerprints(post_fingerprints, post_idx);
                }
                AddToTransaction(trn_type) => {
                    let dest_trn_idx = match trn_type {
                        New(pending_trn_idx) => {
                            new_trn_idxs
                                .get(&HashableTransactionIndex(pending_trn_idx))
                                .expect("new transaction index not found in new transaction arena")
                                .0
                        }
                        Existing(trn_idx) => trn_idx,
                    };
                    let post_idx = self.add_posting(post.post, dest_trn_idx);
                    let dest_trn = self.get_trn_mut(dest_trn_idx);
                    dest_trn.postings.push(post_idx);
                }
            }
        }
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

    fn add_transaction_holder(&mut self, trn: TransactionHolder) -> TransactionIndex {
        let date = trn.trn.date;
        let idx = self.trn_arena.insert(trn);
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

struct PendingMerges {
    /// Posts to merge so far.
    posts: Vec<PendingPosting>,
    /// New transactions to create.
    new_trns: TransactionArena,
}

impl PendingMerges {
    fn new() -> Self {
        PendingMerges {
            posts: Vec::new(),
            new_trns: TransactionArena::new(),
        }
    }
}

struct PendingPosting {
    post: InputPosting,
    destination: PostingDestination,
}

enum PostingDestination {
    MergeIntoExisting(PostingIndex),
    AddToTransaction(DestinationTransaction),
}

#[derive(Clone, Copy)]
enum DestinationTransaction {
    Existing(TransactionIndex),
    New(TransactionIndex),
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
    /// Moves trn into a new `TransactionHolder`, moving out any Postings
    /// inside.
    fn from_transaction(mut trn: Transaction) -> (Self, Vec<Posting>) {
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

#[derive(Eq)]
struct HashablePostingIndex(PostingIndex);
impl PartialEq for HashablePostingIndex {
    fn eq(&self, other: &Self) -> bool {
        self.0.arr_idx() == other.0.arr_idx() && self.0.gen() == other.0.gen()
    }
}
impl std::hash::Hash for HashablePostingIndex {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.arr_idx().hash(state);
    }
}

#[derive(Eq)]
struct HashableTransactionIndex(TransactionIndex);
impl PartialEq for HashableTransactionIndex {
    fn eq(&self, other: &Self) -> bool {
        self.0.arr_idx() == other.0.arr_idx() && self.0.gen() == other.0.gen()
    }
}
impl std::hash::Hash for HashableTransactionIndex {
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

    // Postings from a call to merge should not match earlier postings from the
    // same call to merge.
    #[test]
    fn postings_do_not_match_from_same_merge() {
        let mut merger = Merger::new();

        // The tags in the transactions aren't significant for the test, but
        // their merging makes it more obvious what's wrong if there's a
        // failure.
        merger
            .merge(parse_transactions(
                r#"
                2000/01/01 Foo
                    assets:foo  GBP 10.00  ; :foo-1:
                2000/01/01 Foo
                    assets:foo  GBP 10.00  ; :foo-2:
                2000/01/01 Foo
                    assets:foo  GBP 10.00  ; :foo-3:
                2000/01/01 Foo
                    assets:foo  GBP 10.00  ; :foo-4:
                "#,
            ))
            .unwrap();

        let result = merger.build();
        assert_transactions_eq!(
            &result,
            parse_transactions(
                r#"
                2000/01/01 Foo
                    assets:foo  GBP 10.00  ; :foo-1:
                2000/01/01 Foo
                    assets:foo  GBP 10.00  ; :foo-2:
                2000/01/01 Foo
                    assets:foo  GBP 10.00  ; :foo-3:
                2000/01/01 Foo
                    assets:foo  GBP 10.00  ; :foo-4:
                "#
            ),
        );
    }

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
