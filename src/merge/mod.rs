use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use failure::Error;
use ledger_parser::Transaction;

mod posting;
mod transaction;

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(
        display = "input posting matches multiple destination postings with fingerprints: {}",
        fingerprints
    )]
    InputPostingMatchesMultiplePostings { fingerprints: DisplayStringList },
    #[fail(
        display = "input transaction on {} ({:?}) matches multiple existing transactions: {}",
        in_trn_date, in_trn_desc, out_trn_descs
    )]
    InputTransactionMatchesMultipleTransactions {
        in_trn_date: NaiveDate,
        in_trn_desc: String,
        out_trn_descs: DisplayStringList,
    },
    #[fail(
        display = "multiple postings with same fingerprint ({:?}) found within a single input transaction set",
        fingerprint
    )]
    DuplicateFingerprint { fingerprint: String },
}

#[derive(Debug)]
struct DisplayStringList(Vec<String>);
impl std::fmt::Display for DisplayStringList {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        f.write_str(&itertools::join(
            self.0.iter().map(|f| format!("{:?}", f)),
            ", ",
        ))
    }
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

    fn make_pending(&self, orig_trns: Vec<Transaction>) -> Result<PendingMerges, Error> {
        let mut pending = PendingMerges::new();

        // Set of fingerprints found in `pending.posts` so far.
        // This is used to check if duplicate fingerprints exist in the input.
        let mut fingerprints = HashSet::<String>::new();

        for orig_trn in orig_trns.into_iter() {
            let (src_trn, orig_posts) = transaction::Holder::from_transaction(orig_trn);

            if orig_posts.is_empty() {
                continue;
            }

            let src_posts_matched: Vec<(posting::Input, Option<posting::Index>)> = orig_posts
                .into_iter()
                .map(|orig_post| {
                    let src_post = posting::Input::from_posting(orig_post, src_trn.get_date())?;
                    let dest_post: Option<posting::Index> =
                        self.find_matching_posting(&src_post, src_trn.get_date())?;
                    Ok((src_post, dest_post))
                })
                .collect::<Result<Vec<(posting::Input, Option<posting::Index>)>, Error>>()?;

            for (src_post, _) in &src_posts_matched {
                for fp in src_post.fingerprints.iter().cloned() {
                    if fingerprints.contains(&fp) {
                        return Err(MergeError::DuplicateFingerprint { fingerprint: fp }.into());
                    }
                    fingerprints.insert(fp);
                }
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

    fn find_matching_posting(
        &self,
        post: &posting::Input,
        date: NaiveDate,
    ) -> Result<Option<posting::Index>, Error> {
        let fp_idx = self.find_posting_by_fingerprints(post)?;
        if fp_idx.is_some() {
            return Ok(fp_idx);
        }

        // Look for a match based on internal values.
        for candidate_idx in self.posts.date_to_indices(date) {
            let candidate_post = self.posts.get(candidate_idx);
            if candidate_post.matches(post) {
                return Ok(Some(candidate_idx));
            }
        }

        Ok(None)
    }

    /// Look for match by existing fingerprint(s). Matches zero or one postings
    /// on success, multiple matches are an error.
    fn find_posting_by_fingerprints(
        &self,
        post: &posting::Input,
    ) -> Result<Option<posting::Index>, Error> {
        let posts: HashSet<HashablePostingIndex> = post
            .fingerprints
            .iter()
            .filter_map(|fp| self.posts.fingerprint_to_index(fp))
            .map(HashablePostingIndex)
            .collect();
        match posts.len() {
            n if n <= 1 => Ok(posts.iter().nth(0).map(|i| i.0)),
            _ => Err(MergeError::InputPostingMatchesMultiplePostings {
                fingerprints: DisplayStringList(post.fingerprints.clone()),
            }
            .into()),
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
            _ => Err(MergeError::InputTransactionMatchesMultipleTransactions {
                in_trn_date: src_trn.get_date(),
                in_trn_desc: src_trn.get_description().to_string(),
                out_trn_descs: DisplayStringList(
                    candidate_trns
                        .iter()
                        .map(|trn_idx| self.trns.get(trn_idx.0).get_description().to_string())
                        .collect(),
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
struct HashablePostingIndex(posting::Index);
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
    fn many_matched_transactions_failure() {
        let mut merger = Merger::new();
        merger
            .merge(parse_transactions(
                r#"
                2000/01/01 Transfer to checking
                    assets:checking  GBP 100.00
                2000/01/01 Transfer to savings
                    assets:savings   GBP 100.00
                "#,
            ))
            .unwrap();
        assert!(merger
            .merge(parse_transactions(
                // The existing transactions have postings that match both the
                // postings from the single input transaction.
                r#"
                2000/01/01 Mixed
                    assets:checking  GBP 100.00
                    assets:savings   GBP 100.00
                "#,
            ))
            .is_err());
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
}
