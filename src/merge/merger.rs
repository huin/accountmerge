use std::collections::{HashMap, HashSet};

use failure::Error;
use ledger_parser::{Posting, Transaction};

use crate::merge::matchset::MatchSet;
use crate::merge::{posting, transaction, MergeError};
use crate::mutcell::MutCell;
use crate::tags;

/// A newtype to return transactions that failed to merge and that need human
/// intervention to resolve.
pub struct UnmatchedTransactions(pub Vec<Transaction>);

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
    pub fn merge(&mut self, src_trns: Vec<Transaction>) -> Result<UnmatchedTransactions, Error> {
        let pending = self.make_pending(src_trns)?;
        self.check_pending(&pending)?;
        self.apply_pending(pending)
    }

    fn make_pending(&self, orig_trns: Vec<Transaction>) -> Result<PendingMerges, Error> {
        let mut pending = PendingMerges::new();

        // Set of fingerprints found in `pending.posts` so far.
        // This is used to check if duplicate fingerprints exist in the input.
        let mut fingerprints_seen = HashSet::<String>::new();

        for orig_trn in orig_trns.into_iter() {
            self.add_trn_to_pending(&mut fingerprints_seen, &mut pending, orig_trn)?;
        }

        Ok(pending)
    }

    fn check_pending(&self, pending: &PendingMerges) -> Result<(), Error> {
        // Check if multiple source postings have matched against the same
        // destination posting.
        {
            let mut src_idx_by_dest: HashMap<posting::IndexHashable, MatchSet<usize>> =
                HashMap::new();
            for (pending_post_idx, pending_post) in pending.posts.iter().enumerate() {
                use PostingDestination::*;
                match pending_post.destination {
                    MergeIntoExisting(existing_post_idx) => {
                        let key = posting::IndexHashable(existing_post_idx);
                        src_idx_by_dest
                            .entry(key)
                            .or_default()
                            .insert(pending_post_idx);
                    }
                    // No possible conflict.
                    AddToTransaction(_) => {}
                }
            }

            for (dest_idx_hash, src_idxs) in src_idx_by_dest {
                if src_idxs.len() > 1 {
                    let inputs = itertools::join(
                        src_idxs.iter().map(|src_idx| {
                            format!("{}", pending.posts[*src_idx].post.get_posting())
                        }),
                        "\n",
                    );
                    let destination = self.posts.get(dest_idx_hash.0);
                    let reason = format!(
                        "{} input postings match the same destination posting\ninputs:\n{}\n\ndestination:\n{}",
                        src_idxs.len(),
                        inputs,
                        destination.get_posting(),
                    );
                    return Err(MergeError::Input { reason }.into());
                }
            }
        }

        Ok(())
    }

    fn apply_pending(
        &mut self,
        mut pending: PendingMerges,
    ) -> Result<UnmatchedTransactions, Error> {
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
                    self.posts.merge_into(existing_post_idx, post.post)?;
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
                    let post_idx = self.posts.add(post.post, dest_trn_idx)?;
                    self.trns.add_post_to_trn(dest_trn_idx, post_idx);
                }
            }
        }

        Ok(pending.unmerged_trns)
    }

    fn add_trn_to_pending(
        &self,
        fingerprints_seen: &mut HashSet<String>,
        pending: &mut PendingMerges,
        orig_trn: Transaction,
    ) -> Result<(), Error> {
        let (src_trn, orig_posts) = transaction::Holder::from_transaction(orig_trn);

        if orig_posts.is_empty() {
            return Ok(());
        }

        let mut src_post_actions = MergeActions::new();
        for orig_post in orig_posts.into_iter() {
            let mut src_post = posting::Input::from_posting(orig_post, src_trn.get_date())?;

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
            MergeActionsInner::LeaveUnmerged(input_postings) => {
                // src_trn is to remain unmerged for a human to handle
                // remaining problems.
                let postings: Vec<Posting> = input_postings
                    .into_iter()
                    .map(posting::Input::into_posting)
                    .collect();
                let trn = src_trn.into_transaction(postings);
                pending.unmerged_trns.0.push(trn);
            }
            MergeActionsInner::Actions(src_post_actions) => {
                // Determine default destination transaction.
                let default_dest_trn: DestinationTransaction = self
                    .find_existing_dest_trn(&src_trn, &src_post_actions)?
                    .map(DestinationTransaction::Existing)
                    .unwrap_or_else(|| {
                        DestinationTransaction::New(pending.new_trns.insert(src_trn))
                    });

                pending
                    .posts
                    .extend(src_post_actions.into_iter().map(|(src_post, action)| {
                        let destination = action.with_dest_transaction(default_dest_trn);

                        PendingPosting {
                            post: src_post,
                            destination,
                        }
                    }));
            }
        }

        Ok(())
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
                        matched_idxs
                            .iter()
                            .map(|dest_idx| format!("{}", self.posts.get(*dest_idx).get_posting())),
                        "\n",
                    );
                    let reason = format!(
                        "input posting matches multiple same destination postings by fingerprints\ninput:\n{}\nmatched ndestinations:\n{}",
                        src_post.get_posting(),
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
                // No matched posting. Add to the same transaction as the
                // other postings.
                Ok(Some(AddToTransaction))
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
                    MergeIntoExisting(dest_post_idx) => Some(*dest_post_idx),
                    AddToTransaction => None,
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

/// Accumulates pairs of `posting::Input` and the chosen `PostingMergeAction`
/// for it, until a `None` is added, at which point it throws away the
/// current and future `PostingMergeAction`s.
struct MergeActions(MutCell<MergeActionsInner>);

impl MergeActions {
    fn new() -> Self {
        Self(MutCell::new(MergeActionsInner::Actions(Vec::new())))
    }

    fn push(&mut self, posting: posting::Input, action: Option<PostingMergeAction>) {
        self.0.map_value(|inner| {
            use MergeActionsInner::*;
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

    fn into_inner(self) -> MergeActionsInner {
        self.0.into_inner()
    }
}

enum MergeActionsInner {
    /// Merge/add the postings into the destination.
    Actions(Vec<(posting::Input, PostingMergeAction)>),
    /// Leave the postings unmerged for a human to resolve.
    LeaveUnmerged(Vec<posting::Input>),
}

enum PostingMergeAction {
    /// Merge the posting into the specified existing posting.
    MergeIntoExisting(posting::Index),
    /// Add the posting into the same transaction as the other postings.
    AddToTransaction,
}

impl PostingMergeAction {
    fn with_dest_transaction(self, dest_trn: DestinationTransaction) -> PostingDestination {
        use PostingDestination::*;
        match self {
            Self::MergeIntoExisting(dest_post_idx) => MergeIntoExisting(dest_post_idx),
            Self::AddToTransaction => AddToTransaction(dest_trn),
        }
    }
}

struct PendingMerges {
    /// Posts to merge so far.
    posts: Vec<PendingPosting>,
    /// New transactions to create.
    new_trns: transaction::Arena,
    /// Transactions that failed to merge.
    unmerged_trns: UnmatchedTransactions,
}

impl PendingMerges {
    fn new() -> Self {
        PendingMerges {
            posts: Vec::new(),
            new_trns: transaction::Arena::new(),
            unmerged_trns: UnmatchedTransactions(Vec::new()),
        }
    }
}

struct PendingPosting {
    post: posting::Input,
    destination: PostingDestination,
}

enum PostingDestination {
    /// Merge the posting into the specified existing posting.
    MergeIntoExisting(posting::Index),
    // Create new posting, added to the default destination transaction.
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
        assert!(merger.merge(parse_transactions(first)).is_err());
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
        let unmerged = merger.merge(parse_transactions(first)).unwrap();
        assert!(unmerged.0.is_empty());
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

        let unmerged = merger.merge(parse_transactions(first)).unwrap();
        assert!(unmerged.0.is_empty());

        let result = merger.build();
        assert_transactions_eq!(&result, parse_transactions(want));
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

        let unmerged_first = merger.merge(parse_transactions(first)).unwrap();
        assert!(unmerged_first.0.is_empty());

        let unmerged_second = merger.merge(parse_transactions(second)).unwrap();
        assert_transactions_eq!(unmerged_second.0, parse_transactions(want_unmerged_second));

        let result = merger.build();
        assert_transactions_eq!(&result, parse_transactions(want));
    }
}
