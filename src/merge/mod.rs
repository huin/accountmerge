use std::collections::{HashMap, HashSet};

use failure::Error;
use ledger_parser::Transaction;

use crate::tags;

mod matchset;
mod posting;
mod transaction;

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(display = "bad input to merge: {}", reason)]
    Input { reason: String },
}

pub struct Merger {
    posts: posting::IndexedPostings,
    trns: transaction::IndexedTransactions,
}

impl Merger {
    pub fn new() -> Self {
        Merger {
            posts: posting::IndexedPostings::new(),
            trns: transaction::IndexedTransactions::new(),
        }
    }

    /// This merging algorithm is described in README.md under "Matching
    /// algorithm".
    pub fn merge(&mut self, src_trns: Vec<Transaction>) -> Result<(), Error> {
        let pending = self.make_pending(src_trns)?;
        self.apply_pending(pending);
        Ok(())
    }

    fn make_pending(&mut self, orig_trns: Vec<Transaction>) -> Result<PendingMerges, Error> {
        let mut pending = PendingMerges::new();

        // Set of fingerprints found in `pending.posts` so far.
        // This is used to check if duplicate fingerprints exist in the input.
        let mut fingerprints = HashSet::<String>::new();

        for orig_trn in orig_trns.into_iter() {
            let (src_trn, orig_posts) = transaction::Holder::from_transaction(orig_trn);

            if orig_posts.is_empty() {
                continue;
            }

            let mut src_posts_matched =
                Vec::<(posting::Input, Option<posting::Index>)>::with_capacity(orig_posts.len());
            for orig_post in orig_posts.into_iter() {
                let src_post = posting::Input::from_posting(orig_post, src_trn.get_date());

                for fp in src_post.iter_fingerprints().map(str::to_string) {
                    if fingerprints.contains(&fp) {
                        return Err(MergeError::Input{reason: format!("multiple postings with same fingerprint ({:?}) found within a single input transaction set", fp)}.into());
                    }
                    fingerprints.insert(fp);
                }

                use posting::Match::*;
                use posting::MatchedIndices::*;
                match self.posts.find_matching_postings(&src_post) {
                    Fingerprint(m) => match m {
                        One(dest_idx) => {
                            // Unambiguous match by fingerprint.
                            src_posts_matched.push((src_post, Some(dest_idx)));
                        }
                        Many(_matched_idxs) => {
                            // TODO: Use `_matched_idxs` to improve the error
                            // message with fingerprints from the matches.
                            // Multiple destinations postings matched the
                            // fingerprint(s) of the input posting, this is a
                            // fatal merge error.
                            return Err(MergeError::Input{reason: format!("input posting with fingerprints {} matches multiple destination postings",
                                &itertools::join(src_post.iter_fingerprints(), ", "))}
                            .into());
                        }
                    },
                    Soft(m) => match m {
                        One(dest_idx) => {
                            // Unambiguous single soft match.
                            src_posts_matched.push((src_post, Some(dest_idx)));
                        }
                        Many(matched_idxs) => {
                            // Ambiguous multiple possible soft matches.
                            // Add candidate tags and don't use src_post any further.
                            let fp = src_post.iter_fingerprints().nth(0).map(str::to_string);
                            match fp {
                                None => {
                                    return Err(MergeError::Input {
                                        reason: format!("posting {} has no fingerprints, required for candidate matching", src_post.into_posting()),
                                    }
                                    .into());
                                }
                                Some(fp) => {
                                    let candidate_tag =
                                        format!("{}{}", tags::CANDIDATE_FP_TAG_PREFIX, fp);
                                    for idx in matched_idxs.into_iter() {
                                        self.posts
                                            .get_mut(idx)
                                            .comment
                                            .tags
                                            .insert(candidate_tag.clone());
                                    }
                                }
                            }
                        }
                    },
                    Zero => {
                        src_posts_matched.push((src_post, None));
                    }
                }
            }

            if src_posts_matched.is_empty() {
                // No posts matched, so no further work to do.
                continue;
            }

            // Determine default destination transaction.
            let default_dest_trn: DestinationTransaction = self
                .find_existing_dest_trn(&src_trn, &src_posts_matched)?
                .map(DestinationTransaction::Existing)
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
                    HashableTransactionIndex(self.trns.add(trn)),
                )
            })
            .collect();

        for post in pending.posts.drain(..) {
            use DestinationTransaction::*;
            use PostingDestination::*;

            match post.destination {
                MergeIntoExisting(existing_post_idx) => {
                    self.posts.merge_into(existing_post_idx, post.post);
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
                    let post_idx = self.posts.add(post.post, dest_trn_idx);
                    self.trns.add_post_to_trn(dest_trn_idx, post_idx);
                }
            }
        }
    }

    /// Gethers the existing transactions that are the parents of the
    /// `src_posts_matched`. Returns None if `src_posts_matched` contains no
    /// postings. Returns an error if multiple transactions are parents of the
    /// `src_posts_matched`.
    fn find_existing_dest_trn(
        &self,
        src_trn: &transaction::Holder,
        src_posts_matched: &[(posting::Input, Option<posting::Index>)],
    ) -> Result<Option<transaction::Index>, Error> {
        let candidate_trns: HashSet<HashableTransactionIndex> = src_posts_matched
            .iter()
            .filter_map(|(_, opt_dest_post)| {
                opt_dest_post
                    .map(|dest_post| self.posts.get(dest_post).get_parent_trn())
                    .map(HashableTransactionIndex)
            })
            .collect();

        // Check that only one destination transaction matches.
        match candidate_trns.len() {
            n if n <= 1 => Ok(candidate_trns.iter().nth(0).map(|i| i.0)),
            _ => Err(MergeError::Input {
                reason: format!(
                    "input transaction on {} ({:?}) matches multiple existing transactions: {}",
                    src_trn.get_date(),
                    src_trn.get_description().to_string(),
                    itertools::join(
                        candidate_trns
                            .iter()
                            .map(|trn_idx| self.trns.get(trn_idx.0).get_description()),
                        ", "
                    ),
                ),
            }
            .into()),
        }
    }

    pub fn build(self) -> Vec<Transaction> {
        let mut posts = self.posts.into_consume();

        let mut out = Vec::<Transaction>::new();
        for trn_holder in self.trns.into_iter() {
            let posts = trn_holder
                .iter_posting_indices()
                .map(|post_idx| posts.take(post_idx))
                .collect();
            let trn = trn_holder.into_transaction(posts);
            out.push(trn);
        }

        out
    }
}

struct PendingMerges {
    /// Posts to merge so far.
    posts: Vec<PendingPosting>,
    /// New transactions to create.
    new_trns: transaction::Arena,
}

impl PendingMerges {
    fn new() -> Self {
        PendingMerges {
            posts: Vec::new(),
            new_trns: transaction::Arena::new(),
        }
    }
}

struct PendingPosting {
    post: posting::Input,
    destination: PostingDestination,
}

enum PostingDestination {
    MergeIntoExisting(posting::Index),
    AddToTransaction(DestinationTransaction),
}

#[derive(Clone, Copy)]
enum DestinationTransaction {
    Existing(transaction::Index),
    New(transaction::Index),
}

#[derive(Eq)]
struct HashableTransactionIndex(transaction::Index);
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
    use test_case::test_case;

    use super::*;
    use crate::testutil::parse_transactions;

    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:
            2000/01/02 Transfer to savings
                assets:savings   GBP 100.00   ; :fp-2:
        "#,
        // This posting has fingerprints matching two different postings
        // and should cause an error when atttempting to merge.
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:fp-2:
        "#;
        "fingerprint_many_match_failure"
    )]
    #[test_case(
        r#"
            2000/01/01 Transfer to checking
                assets:checking  GBP 100.00
            2000/01/01 Transfer to savings
                assets:savings   GBP 100.00
        "#,
        // The existing transactions have postings that match both the
        // postings from the single input transaction.
        r#"
            2000/01/01 Mixed
                assets:checking  GBP 100.00
                assets:savings   GBP 100.00
        "#;
        "many_matched_transactions_failure"
    )]
    fn merge_merge_error(first: &str, second: &str) {
        let mut merger = Merger::new();
        merger.merge(parse_transactions(first)).unwrap();
        assert!(merger.merge(parse_transactions(second)).is_err());

        // The result should be the same as before attempting to merge the
        // second time.
        let mut merger_only_first = Merger::new();
        merger_only_first.merge(parse_transactions(first)).unwrap();

        let result = merger.build();
        let only_first = merger_only_first.build();
        assert_transactions_eq!(&result, &only_first);
    }

    #[test_case(
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
        "#;
        "stable_sorts_destination_by_date"
    )]
    #[test_case(
        // Postings from a call to merge should not match earlier postings from the
        // same call to merge.
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
        r#"
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :foo-1:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :foo-2:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :foo-3:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :foo-4:
        "#;
        "postings_do_not_match_from_same_merge"
    )]
    fn merge_build(first: &str, want: &str) {
        let mut merger = Merger::new();

        merger.merge(parse_transactions(first)).unwrap();
        let result = merger.build();
        assert_transactions_eq!(&result, parse_transactions(want),);
    }

    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            2000/01/02 Lunch
                assets:checking  GBP -5.00
                expenses:dining  GBP 5.00
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            2000/01/02 Lunch
                assets:checking  GBP -5.00
                expenses:dining  GBP 5.00
        "#;
        "dedupes_added"
    )]
    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:fp-2:fp-3:
        "#,
        // Different date to avoid soft-matching if fingerprint matching fails.
        r#"
            2000/01/02 Salary
                assets:checking  GBP 100.00   ; :fp-1:fp-2:fp-4:
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  ; :fp-1:fp-2:fp-3:fp-4:
        "#;
        "fingerprint_matching"
    )]
    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-orig1:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-orig2:
            2000/01/01 Salary
                assets:checking  GBP 100.00
        "#,
        // These postings all soft-match against the postings above,
        // but will *not* be merged in.
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-new1:foo:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-new2:bar:
        "#,
        // Candidate match tags should be added, but not the :foo: and :bar:
        // tags.
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :candidate-fp-new1:candidate-fp-new2:fp-orig1:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :candidate-fp-new1:candidate-fp-new2:fp-orig2:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :candidate-fp-new1:candidate-fp-new2:
        "#;
        "soft_posting_match_adds_candidate_match_tags"
    )]
    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  =GBP 1234.00
                income:salary    GBP -100.00
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  = GBP 1234.00
                income:salary    GBP -100.00
        "#;
        "balances_added_to_existing"
    )]
    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  =GBP 1234.00
                income:salary    GBP -100.00
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  = GBP 1234.00
                income:salary    GBP -100.00
        "#;
        "does_not_overwrite_some_fields"
    )]
    fn merge_merge_build(first: &str, second: &str, want: &str) {
        let mut merger = Merger::new();

        merger.merge(parse_transactions(first)).unwrap();
        merger.merge(parse_transactions(second)).unwrap();
        let result = merger.build();
        assert_transactions_eq!(&result, parse_transactions(want),);
    }
}
