use std::collections::{HashMap, HashSet};

use failure::Error;

use crate::internal::{PostingInternal, TransactionPostings};
use crate::merge::{posting, transaction, MergeError};
use crate::mutcell::MutCell;
use crate::tags;

/// A newtype to return transactions that failed to merge and that need human
/// intervention to resolve.
pub struct UnmergedTransactions(pub Vec<TransactionPostings>);

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
    pub fn merge(
        &mut self,
        src_trns: Vec<TransactionPostings>,
    ) -> Result<UnmergedTransactions, Error> {
        let pending = self.make_pending(src_trns)?;
        self.check_pending(&pending)?;
        self.apply_pending(pending)
    }

    fn make_pending(
        &self,
        orig_trns: Vec<TransactionPostings>,
    ) -> Result<Vec<TransactionMergeAction>, Error> {
        let mut pending = Vec::<TransactionMergeAction>::new();

        // Set of fingerprints found in `pending.posts` so far.
        // This is used to check if duplicate fingerprints exist in the input.
        let mut fingerprints_seen = HashSet::<String>::new();

        for orig_trn in orig_trns.into_iter() {
            let trn_action = self.to_transaction_merge_action(&mut fingerprints_seen, orig_trn)?;
            pending.push(trn_action);
        }

        Ok(pending)
    }

    fn check_pending(&self, pending: &[TransactionMergeAction]) -> Result<(), Error> {
        // Check if multiple source postings have matched against the same
        // destination posting.
        // TODO: Should we do the same for merging into the same destination
        // transaction, or is that acceptable, given that we're checking the
        // postings?
        {
            let mut src_idx_by_dest: HashMap<posting::IndexHashable, Vec<&posting::Input>> =
                HashMap::new();
            for trn_action in pending.iter() {
                match trn_action {
                    TransactionMergeAction::New(_) => {
                        // No possible conflict; not merging any child posting
                        // into existing posting.
                    }
                    TransactionMergeAction::MergeInto { pending_trn, .. } => {
                        for (post, action) in &pending_trn.post_actions {
                            match action {
                                PostingMergeAction::New => {
                                    // No possible conflict; not merging this
                                    // posting into an existing posting.
                                }
                                PostingMergeAction::MergeIntoExisting(dest_idx) => {
                                    let dest_idx_hash = posting::IndexHashable(*dest_idx);
                                    src_idx_by_dest.entry(dest_idx_hash).or_default().push(post);
                                }
                            }
                        }
                    }
                    TransactionMergeAction::LeaveUnmerged(_) => {
                        // No possible conflict; not merging this transaction
                        // or its postings at all.
                    }
                }
            }

            for (dest_idx_hash, src_posts) in src_idx_by_dest {
                if src_posts.len() > 1 {
                    // Oh no! Multiple input postings have matched the same
                    // destination transaction.
                    let inputs = itertools::join(
                        src_posts
                            .iter()
                            .map(|src_post| format!("{}", src_post.posting.clone_into_posting())),
                        "\n",
                    );
                    let destination = self.posts.get(dest_idx_hash.0);
                    let reason = format!(
                        "{} input postings match the same destination posting\ninputs:\n{}\n\ndestination:\n{}",
                        src_posts.len(),
                        inputs,
                        destination.posting.clone_into_posting(),
                    );
                    return Err(MergeError::Input { reason }.into());
                }
            }
        }

        Ok(())
    }

    fn apply_pending(
        &mut self,
        pending: Vec<TransactionMergeAction>,
    ) -> Result<UnmergedTransactions, Error> {
        let mut unmerged = Vec::<TransactionPostings>::new();

        for trn_action in pending.into_iter() {
            use TransactionMergeAction::*;

            match trn_action {
                New(pending_trn) => {
                    let dest_trn = self.trns.add(pending_trn.src_trn);
                    self.apply_post_actions_to_trn(dest_trn, pending_trn.post_actions)?;
                }
                MergeInto {
                    pending_trn,
                    dest_trn,
                } => {
                    // `src_trn` currently unused.
                    drop(pending_trn.src_trn);
                    self.apply_post_actions_to_trn(dest_trn, pending_trn.post_actions)?;
                }
                LeaveUnmerged(trn) => {
                    unmerged.push(trn);
                }
            }
        }
        Ok(UnmergedTransactions(unmerged))
    }

    fn apply_post_actions_to_trn(
        &mut self,
        dest_trn_idx: transaction::Index,
        post_actions: Vec<(posting::Input, PostingMergeAction)>,
    ) -> Result<(), Error> {
        for (post, action) in post_actions {
            match action {
                PostingMergeAction::New => {
                    let post_idx = self.posts.add(post, dest_trn_idx)?;
                    self.trns.add_post_to_trn(dest_trn_idx, post_idx);
                }
                PostingMergeAction::MergeIntoExisting(dest_post_idx) => {
                    self.posts.merge_into(dest_post_idx, post)?;
                }
            }
        }
        Ok(())
    }

    fn to_transaction_merge_action(
        &self,
        fingerprints_seen: &mut HashSet<String>,
        orig_trn_postings: TransactionPostings,
    ) -> Result<TransactionMergeAction, Error> {
        if orig_trn_postings.posts.is_empty() {
            // Because we have no postings to match against, we can't merge into
            // an existing transaction. But if we try to create a new
            // transaction, it won't match itself next time a merge happens, and
            // we'll keep added transactions.
            // This could change if we put fingerprints on transactions as well.
            return Ok(TransactionMergeAction::LeaveUnmerged(orig_trn_postings));
        }

        let (orig_trn, orig_posts) = (orig_trn_postings.trn, orig_trn_postings.posts);
        let src_trn = transaction::Holder::from_transaction_internal(orig_trn);

        let mut src_post_actions = MergeActionsAccumulator::new();
        for orig_post in orig_posts.into_iter() {
            let mut src_post =
                posting::Input::from_posting_internal(orig_post, src_trn.trn.raw.date)?;

            for fp in src_post.iter_fingerprints().map(str::to_string) {
                if fingerprints_seen.contains(&fp) {
                    return Err(MergeError::Input{reason: format!("multiple postings with same fingerprint ({:?}) found within a single input transaction set", fp)}.into());
                }
                fingerprints_seen.insert(fp);
            }

            let action = self.determine_posting_action(&mut src_post)?;
            src_post_actions.push(src_post, action);
        }

        match src_post_actions.into_inner() {
            MergeActions::LeaveUnmerged(input_postings) => {
                // src_trn is to remain unmerged for a human to handle
                // remaining problems.
                let postings: Vec<PostingInternal> = input_postings
                    .into_iter()
                    .map(posting::Input::into_posting_internal)
                    .collect();
                Ok(TransactionMergeAction::LeaveUnmerged(
                    src_trn.into_transaction_postings(postings),
                ))
            }
            MergeActions::Actions(src_post_actions) => {
                // Determine default destination transaction.
                let opt_dest_trn: Option<transaction::Index> =
                    self.find_existing_dest_trn(&src_trn, &src_post_actions)?;

                let pending_trn = PendingTransaction {
                    src_trn,
                    post_actions: src_post_actions,
                };

                Ok(match opt_dest_trn {
                    Some(dest_trn) => TransactionMergeAction::MergeInto {
                        pending_trn,
                        dest_trn,
                    },
                    None => TransactionMergeAction::New(pending_trn),
                })
            }
        }
    }

    fn determine_posting_action(
        &self,
        src_post: &mut posting::Input,
    ) -> Result<Option<PostingMergeAction>, Error> {
        use posting::Match::*;
        use posting::MatchedIndices::*;
        use PostingMergeAction::*;
        match self.posts.find_matching_postings(&src_post) {
            Fingerprint(m) => match m {
                One(dest_idx) => {
                    // Unambiguous match by fingerprint.
                    Ok(Some(MergeIntoExisting(dest_idx)))
                }
                Many(matched_idxs) => {
                    // Multiple destinations postings matched the
                    // fingerprint(s) of the input posting, this is a
                    // fatal merge error.
                    let destinations = itertools::join(
                        matched_idxs.iter().map(|dest_idx| {
                            format!("{}", self.posts.get(*dest_idx).posting.clone_into_posting())
                        }),
                        "\n",
                    );
                    let reason = format!(
                        "input posting matches multiple same destination postings by fingerprints\ninput:\n{}\nmatched ndestinations:\n{}",
                        src_post.posting.clone_into_posting(),
                        destinations,
                    );
                    Err(MergeError::Input { reason }.into())
                }
            },

            Soft(m) => match m {
                One(dest_idx) => {
                    // Unambiguous single soft match.
                    Ok(Some(MergeIntoExisting(dest_idx)))
                }
                Many(matched_idxs) => {
                    // Add candidate tags of the destinations to the
                    // single src_post and mark the entire transaction
                    // as unmerged.
                    for idx in matched_idxs.into_iter() {
                        let candidate_dest_post = self.posts.get(idx);
                        src_post.add_tag(format!(
                            "{}{}",
                            tags::CANDIDATE_FP_TAG_PREFIX,
                            candidate_dest_post.primary_fingerprint()
                        ));
                    }
                    // No clear matched posting, let a human decide what action
                    // to take.
                    Ok(None)
                }
            },

            Zero => {
                // No matched posting. Add as a new posting.
                Ok(Some(New))
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
        src_posts_matched: &[(posting::Input, PostingMergeAction)],
    ) -> Result<Option<transaction::Index>, Error> {
        // Look for parent transactions of postings that have been matched as
        // destination postings.
        let candidate_trns: HashSet<HashableTransactionIndex> = src_posts_matched
            .iter()
            .filter_map(|(_, action)| {
                use PostingMergeAction::*;
                match action {
                    New => None,
                    MergeIntoExisting(dest_post_idx) => Some(*dest_post_idx),
                }
            })
            .map(|dest_post_idx| self.posts.get(dest_post_idx).get_parent_trn())
            .map(HashableTransactionIndex)
            .collect();

        // Check that only one destination transaction matches.
        match candidate_trns.len() {
            n if n <= 1 => Ok(candidate_trns.iter().nth(0).map(|i| i.0)),
            _ => Err(MergeError::Input {
                reason: format!(
                    "input transaction on {} ({:?}) matches multiple existing transactions: {}",
                    src_trn.trn.raw.date,
                    src_trn.trn.raw.description,
                    itertools::join(
                        candidate_trns.iter().map(|trn_idx| &self
                            .trns
                            .get(trn_idx.0)
                            .trn
                            .raw
                            .description),
                        ", "
                    ),
                ),
            }
            .into()),
        }
    }

    pub fn build(self) -> Vec<TransactionPostings> {
        let mut posts = self.posts.into_consume();

        let mut out = Vec::<TransactionPostings>::new();
        for trn_holder in self.trns.into_iter() {
            let posts = trn_holder
                .iter_posting_indices()
                .map(|post_idx| posts.take(post_idx))
                .collect();
            let trn = trn_holder.into_transaction_postings(posts);
            out.push(trn);
        }

        out
    }
}

/// Accumulates pairs of `posting::Input` and the chosen `PostingMergeAction`
/// for it, until a `None` is added, at which point it throws away the
/// current and future `PostingMergeAction`s.
struct MergeActionsAccumulator(MutCell<MergeActions>);

impl MergeActionsAccumulator {
    fn new() -> Self {
        Self(MutCell::new(MergeActions::Actions(Vec::new())))
    }

    fn push(&mut self, posting: posting::Input, action: Option<PostingMergeAction>) {
        self.0.map_value(|inner| {
            use MergeActions::*;
            match inner {
                Actions(mut post_actions) => match action {
                    Some(action) => {
                        post_actions.push((posting, action));
                        Actions(post_actions)
                    }
                    None => {
                        let mut postings: Vec<posting::Input> = post_actions
                            .into_iter()
                            .map(|(post, _action)| post)
                            .collect();
                        postings.push(posting);
                        LeaveUnmerged(postings)
                    }
                },
                LeaveUnmerged(mut postings) => {
                    postings.push(posting);
                    LeaveUnmerged(postings)
                }
            }
        });
    }

    fn into_inner(self) -> MergeActions {
        self.0.into_inner()
    }
}

enum MergeActions {
    /// Merge/add the postings into the destination.
    Actions(Vec<(posting::Input, PostingMergeAction)>),
    /// Leave the postings unmerged for a human to resolve.
    LeaveUnmerged(Vec<posting::Input>),
}

enum PostingMergeAction {
    /// Create new posting based on the source posting.
    New,
    /// Merge the posting into the existing posting.
    MergeIntoExisting(posting::Index),
}

struct PendingTransaction {
    src_trn: transaction::Holder,
    post_actions: Vec<(posting::Input, PostingMergeAction)>,
}

enum TransactionMergeAction {
    /// Create a new transaction based on the source transaction.
    New(PendingTransaction),
    /// Merge the postings into the existing transaction.
    MergeInto {
        pending_trn: PendingTransaction,
        dest_trn: transaction::Index,
    },
    /// Leave the transaction unmerged for a human to handle.
    LeaveUnmerged(TransactionPostings),
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
    use crate::testutil::parse_transaction_postings;

    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
        "#;
        "error_when_merging_without_fingerprint"
    )]
    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  ; :fp-1:candidate-fp-2:
        "#;
        "merging_with_candidate_tag"
    )]
    fn merge_error(first: &str) {
        let mut merger = Merger::new();
        assert!(merger.merge(parse_transaction_postings(first)).is_err());
    }

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
        "posts_fingerprint_match_multiple_posts"
    )]
    #[test_case(
        r#"
            2000/01/01 Transfer to checking
                assets:checking  GBP 100.00  ; :fp-1:
            2000/01/01 Transfer to savings
                assets:savings   GBP 100.00  ; :fp-2:
        "#,
        // The existing transactions have postings that match both the
        // postings from the single input transaction.
        r#"
            2000/01/01 Mixed
                assets:checking  GBP 100.00  ; :fp-1:
                assets:savings   GBP 100.00  ; :fp-2:
        "#;
        "transation_would_be_split"
    )]
    #[test_case(
        r#"
            2000/01/01 Foo
                assets:checking  GBP 100.00  ; :fp-1:fp-2:
        "#,
        r#"
            2000/01/01 Foo-1
                assets:checking  GBP 100.00  ; :fp-1:
            2000/01/01 Foo-2
                assets:checking  GBP 100.00  ; :fp-2:
        "#;
        "multiple_postings_match_same_destination"
    )]
    fn merge_merge_error(first: &str, second: &str) {
        let mut merger = Merger::new();
        let unmerged = merger.merge(parse_transaction_postings(first)).unwrap();
        assert!(unmerged.0.is_empty());
        assert!(merger.merge(parse_transaction_postings(second)).is_err());

        // The result should be the same as before attempting to merge the
        // second time.
        let mut merger_only_first = Merger::new();
        merger_only_first
            .merge(parse_transaction_postings(first))
            .unwrap();

        let result = merger.build();
        let only_first = merger_only_first.build();
        assert_transaction_postings_eq!(result, only_first);
    }

    #[test_case(
        r#"
            2000/02/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:
                income:salary    GBP -100.00  ; :fp-2:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-3:
                income:salary    GBP -100.00  ; :fp-4:
            2000/02/01 Lunch
                assets:checking  GBP -5.00    ; :fp-5:
                expenses:dining  GBP 5.00     ; :fp-6:
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-3:
                income:salary    GBP -100.00  ; :fp-4:
            2000/02/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:
                income:salary    GBP -100.00  ; :fp-2:
            2000/02/01 Lunch
                assets:checking  GBP -5.00    ; :fp-5:
                expenses:dining  GBP 5.00     ; :fp-6:
        "#;
        "stable_sorts_by_date"
    )]
    #[test_case(
        // Postings from a call to merge should not match earlier postings from the
        // same call to merge.
        r#"
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :fp-1:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :fp-2:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :fp-3:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :fp-4:
        "#,
        r#"
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :fp-1:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :fp-2:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :fp-3:
            2000/01/01 Foo
                assets:foo  GBP 10.00  ; :fp-4:
        "#;
        "postings_do_not_match_from_same_merge"
    )]
    fn merge_build(first: &str, want: &str) {
        let mut merger = Merger::new();

        let unmerged = merger.merge(parse_transaction_postings(first)).unwrap();
        assert!(unmerged.0.is_empty());

        let result = merger.build();
        assert_transaction_postings_eq!(result, parse_transaction_postings(want));
    }

    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:
                income:salary    GBP -100.00  ; :fp-2:
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-3:
                income:salary    GBP -100.00  ; :fp-4:
            2000/01/02 Lunch
                assets:checking  GBP -5.00    ; :fp-5:
                expenses:dining  GBP 5.00     ; :fp-6:
        "#,
        r#""#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:fp-3:
                income:salary    GBP -100.00  ; :fp-2:fp-4:
            2000/01/02 Lunch
                assets:checking  GBP -5.00    ; :fp-5:
                expenses:dining  GBP 5.00     ; :fp-6:
        "#;
        "soft_matches_existing"
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
        r#""#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:fp-2:fp-3:fp-4:
        "#;
        "fingerprint_matches_existing_non_soft_match"
    )]
    #[test_case(
        r#"
            2000/01/01 Salary
                income:salary    GBP-100.00   ; :fp-sal1:
                assets:checking  GBP 100.00   ; :fp-orig1:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-orig2:
                income:salary    GBP-100.00   ; :fp-sal2:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-orig3:
                income:salary    GBP-100.00   ; :fp-sal3:
        "#,
        // The following assets:checking postings each soft-match against
        // multiple postings above, but should *not* be merged in due to that
        // ambiguity.
        // This also checks if encountering the ambiguity on the first or
        // subsequent posting works.
        r#"
            2000/01/01 Salary
                income:salary    GBP-100.00   ; :fp-sal1:
                assets:checking  GBP 100.00   ; :fp-new1:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-new2:
                income:salary    GBP-100.00   ; :fp-sal2:
        "#,
        // Candidate destination match tags should be added to the src
        // postings, and they should be left in the unmerged transactions.
        r#"
            2000/01/01 Salary
                income:salary    GBP-100.00   ; :fp-sal1:
                assets:checking  GBP 100.00   ; :candidate-fp-orig1:candidate-fp-orig2:candidate-fp-orig3:fp-new1:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :candidate-fp-orig1:candidate-fp-orig2:candidate-fp-orig3:fp-new2:
                income:salary    GBP-100.00   ; :fp-sal2:
        "#,
        // The original transactions should be unchanged.
        r#"
            2000/01/01 Salary
                income:salary    GBP-100.00   ; :fp-sal1:
                assets:checking  GBP 100.00   ; :fp-orig1:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-orig2:
                income:salary    GBP-100.00   ; :fp-sal2:
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-orig3:
                income:salary    GBP-100.00   ; :fp-sal3:
        "#;
        "ambiguous_soft_match_adds_candidate_tags_and_leaves_unmerged"
    )]
    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  ; :fp-1:
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  =GBP 1234.00  ; :fp-1:
        "#,
        r#""#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  =GBP 1234.00  ; :fp-1:
        "#;
        "balances_added_to_existing"
    )]
    #[test_case(
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00   ; :fp-1:
                income:salary    GBP -100.00  ; :fp-2:
        "#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  =GBP 1234.00  ; :fp-1:
                income:salary    GBP -100.00               ; :fp-2:
        "#,
        r#""#,
        r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00  =GBP 1234.00  ; :fp-1:
                income:salary    GBP -100.00               ; :fp-2:
        "#;
        "does_not_overwrite_some_fields"
    )]
    fn merge_merge_build(first: &str, second: &str, want_unmerged_second: &str, want: &str) {
        let mut merger = Merger::new();

        let unmerged_first = merger.merge(parse_transaction_postings(first)).unwrap();
        assert!(unmerged_first.0.is_empty());

        let unmerged_second = merger.merge(parse_transaction_postings(second)).unwrap();
        assert_transaction_postings_eq!(
            unmerged_second.0,
            parse_transaction_postings(want_unmerged_second)
        );

        let result = merger.build();
        assert_transaction_postings_eq!(result, parse_transaction_postings(want));
    }
}
