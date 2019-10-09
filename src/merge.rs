use std::collections::HashMap;

use chrono::NaiveDate;
use failure::Error;
use ledger_parser::{Posting, Transaction};
use typed_generational_arena::{StandardArena, StandardIndex};

use crate::comment::Comment;
use crate::tags::{CANONICAL_TAG, FINGERPRINT_TAG_PREFIX};

const BAD_POSTING_INDEX: &str = "internal error: used invalid PostingIndex";
const BAD_TRANSACTION_INDEX: &str = "internal error: used invalid TransactionIndex";

type PostingArena = StandardArena<PostingHolder>;
type PostingIndex = StandardIndex<PostingHolder>;
type TransactionArena = StandardArena<TransactionHolder>;
type TransactionIndex = StandardIndex<TransactionHolder>;

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(
        display = "input posting has multiple fingerprint tags:\n{:?}",
        comment
    )]
    InputPostingHasMultipleFingerprints { comment: Comment },
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
                .map(|post| InputPosting::from_posting(post))
                .collect::<Result<Vec<InputPosting>, Error>>()?;
            // dst_posts contains indices to the corresponding existing posts
            // for those in src_posts, or None where no existing matching post
            // was found.
            dest_posts.clear();
            for src_post in &src_posts {
                dest_posts.push(self.find_matching_posting(src_post, src_trn.date.clone()));
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
                        self.register_fingerprint(opt_key, *dest_post_idx);
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
        dest_posts: &Vec<Option<PostingIndex>>,
    ) -> TransactionIndex {
        dest_posts
            .iter()
            .find_map(|opt_dest_post| {
                opt_dest_post.map(|dest_post| self.get_post(dest_post).parent_trn)
            })
            .unwrap_or_else(|| self.add_transaction(src_trn))
    }

    fn find_matching_posting(&self, post: &InputPosting, date: NaiveDate) -> Option<PostingIndex> {
        let fp_idx = self.find_posting_by_fingerprint(post);
        if fp_idx.is_some() {
            return fp_idx;
        }

        // Look for a match based on internal values.
        let opt_post_idxs = self.posts_by_date.get(&date);
        let iter_posts_for_date = opt_post_idxs.iter().flat_map(|idxs| idxs.iter());
        for candidate_idx in iter_posts_for_date {
            let candidate_post = self.get_post(*candidate_idx);
            if candidate_post.matches(post) {
                return Some(*candidate_idx);
            }
        }

        None
    }

    /// Look for match by existing fingerprint.
    fn find_posting_by_fingerprint(&self, post: &InputPosting) -> Option<PostingIndex> {
        post.fingerprint_key
            .as_ref()
            .and_then(|key| self.post_by_fingerprint.get(key))
            .map(|idx| *idx)
    }

    fn add_posting(
        &mut self,
        proto_posting: InputPosting,
        parent_trn: TransactionIndex,
    ) -> PostingIndex {
        let (posting, opt_key) = PostingHolder::from_input_posting(proto_posting, parent_trn);
        let idx = self.post_arena.insert(posting);
        self.register_fingerprint(opt_key, idx);

        self.posts_by_date
            .entry(self.get_trn(parent_trn).trn.date)
            .or_insert_with(Vec::new)
            .push(idx);
        idx
    }

    fn register_fingerprint(
        &mut self,
        opt_fingerprint_key: Option<String>,
        dest_post_idx: PostingIndex,
    ) {
        if let Some(key) = opt_fingerprint_key {
            self.post_by_fingerprint.insert(key, dest_post_idx);
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
                                .to_posting()
                        })
                        .collect();
                    let trn = trn_holder.to_transaction(posts);
                    out.push(trn);
                }
            }
        }

        out
    }
}

/// Creates a key for a `HashMap` from the only fingerprint tag in the given
/// `Comment`. It is an error if there are multiple fingerprint tags, returns
/// `Ok(None)` if there are no fingerprint tags at all.
fn fingerprint_key_from_comment(comment: &Comment) -> Result<Option<String>, Error> {
    let mut key: Option<String> = None;
    for (n, v) in &comment.value_tags {
        match (key.is_some(), fingerprint_key_from_tag(n, v)) {
            (true, Some(_)) => {
                // Found a second key => error.
                return Err(MergeError::InputPostingHasMultipleFingerprints {
                    comment: comment.clone(),
                }
                .into());
            }
            (false, Some(cur_key)) => {
                // Found first key.
                key = Some(cur_key);
            }
            _ => {}
        };
    }
    Ok(key)
}

/// Creates a key for a `HashMap` given a fingerprint `name` and `value`.
/// Returns `None` if `name` is not a fingerprint name.
fn fingerprint_key_from_tag(name: &str, value: &str) -> Option<String> {
    if !name.starts_with(FINGERPRINT_TAG_PREFIX) {
        None
    } else {
        Some(format!("{}:{}", name, value))
    }
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

    fn to_transaction(mut self, postings: Vec<Posting>) -> Transaction {
        self.trn.postings = postings;
        self.trn
    }
}

struct InputPosting {
    fingerprint_key: Option<String>,
    posting: Posting,
    comment: Comment,
}

impl InputPosting {
    fn from_posting(mut posting: Posting) -> Result<Self, Error> {
        let comment = Comment::from_opt_comment(posting.comment.as_ref().map(String::as_str));
        posting.comment = None;
        Ok(Self {
            fingerprint_key: fingerprint_key_from_comment(&comment)?,
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
    ) -> (Self, Option<String>) {
        (
            Self {
                parent_trn,
                posting: proto.posting,
                comment: proto.comment,
            },
            proto.fingerprint_key,
        )
    }

    fn to_posting(mut self) -> Posting {
        self.posting.comment = self.comment.to_opt_comment();
        self.posting
    }

    fn matches(&self, input: &InputPosting) -> bool {
        let b = &input.posting;
        self.posting.account == b.account
            && self.posting.amount == b.amount
            && match (&self.posting.balance, &b.balance) {
                (Some(a_bal), Some(b_bal)) => a_bal == b_bal,
                _ => true,
            }
    }

    fn merge_from_input_posting(&mut self, src: InputPosting) -> Option<String> {
        // TODO: Merge/update status.
        if self.posting.balance.is_none() {
            self.posting.balance = src.posting.balance.clone()
        }
        if !self.comment.tags.contains(CANONICAL_TAG) && src.comment.tags.contains(CANONICAL_TAG) {
            self.posting.account = src.posting.account;
        }
        self.comment.merge_from(src.comment);
        src.fingerprint_key
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
            let (mut holder, _) = PostingHolder::from_input_posting(dest_posting, dummy_idx);
            holder.merge_from_input_posting(src_posting);
            holder.to_posting()
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
    }
}
