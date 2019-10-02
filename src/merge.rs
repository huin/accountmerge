use std::collections::HashMap;

use chrono::NaiveDate;
use ledger_parser::{Posting, Transaction};
use typed_generational_arena::{StandardArena, StandardIndex};

use crate::comment::Comment;

type TransactionArena = StandardArena<TransactionHolder>;
type TransactionIndex = StandardIndex<TransactionHolder>;

pub struct MergeResult {
    pub merged: Vec<Transaction>,
    pub unmerged: Vec<Transaction>,
}

pub struct Merger {
    trn_arena: TransactionArena,
    trn_by_date: HashMap<NaiveDate, Vec<TransactionIndex>>,
    unmerged_trns: Vec<Transaction>,
}

const EMPTY_INDICES: [TransactionIndex; 0] = [];

impl Merger {
    pub fn new() -> Self {
        Merger {
            trn_arena: StandardArena::new(),
            trn_by_date: HashMap::new(),
            unmerged_trns: Vec::new(),
        }
    }

    pub fn merge(&mut self, src: Vec<Transaction>) {
        // Reuse vector allocation in loop (cleared each time).
        let mut candidate_trns = Vec::<TransactionIndex>::new();

        for src_trn in src.into_iter() {
            candidate_trns.clear();

            // Find multiple matching transactions.
            candidate_trns.extend(self.iter_trns_for_date(&src_trn.date).filter(|&trn_index| {
                self.trn_arena
                    .get(trn_index)
                    .expect("bad trn_arena index found in trn_by_date")
                    .all_postings_match_subset(&src_trn)
            }));

            if candidate_trns.len() == 1 {
                let dest_trn = &mut self.trn_arena[candidate_trns[0]];
                for src_posting in &src_trn.postings {
                    if let Some(dest_posting) = dest_trn
                        .postings
                        .iter_mut()
                        .find(|dest_posting| dest_posting.matches(src_posting))
                    {
                        dest_posting.update(src_posting);
                    }
                }
            } else if candidate_trns.len() > 1 {
                self.unmerged_trns.push(src_trn);
            } else {
                self.add_transaction(src_trn);
            }
        }
    }

    fn iter_trns_for_date<'a>(
        &'a self,
        date: &NaiveDate,
    ) -> impl Iterator<Item = TransactionIndex> + 'a {
        self.trn_by_date
            .get(date)
            .map_or_else(|| EMPTY_INDICES.as_ref(), |id_vec| id_vec.as_slice())
            .iter()
            .map(|&index| index)
    }

    fn add_transaction(&mut self, trn: Transaction) -> TransactionIndex {
        let date = trn.date;
        let trn_index = self
            .trn_arena
            .insert(TransactionHolder::from_transaction(trn));
        self.trn_by_date
            .entry(date)
            .or_insert(Vec::new())
            .push(trn_index);
        trn_index
    }

    pub fn build(mut self) -> MergeResult {
        let mut dates: Vec<NaiveDate> = self.trn_by_date.keys().cloned().collect();
        dates.sort();
        let mut trn_by_date = self.trn_by_date;
        let mut out = Vec::<Transaction>::new();

        // Avoid mutably borrowing self twice.
        let mut trn_arena = TransactionArena::new();
        std::mem::swap(&mut trn_arena, &mut self.trn_arena);

        for date in &dates {
            if let Some(date_trn_indices) = trn_by_date.remove(date) {
                for trn_index in date_trn_indices {
                    let trn: Transaction = trn_arena
                        .remove(trn_index)
                        .expect("duplicate or unknown index in date_trn_indices")
                        .to_transaction();
                    out.push(trn);
                }
            }
        }

        MergeResult {
            merged: out,
            unmerged: self.unmerged_trns,
        }
    }
}

/// Contains a partially unpacked `Transaction`.
struct TransactionHolder {
    trn: Transaction,

    postings: Vec<PostingHolder>,
}

impl TransactionHolder {
    fn from_transaction(mut trn: Transaction) -> Self {
        let mut postings_in: Vec<Posting> = Default::default();
        std::mem::swap(&mut postings_in, &mut trn.postings);
        let postings = postings_in
            .into_iter()
            .map(PostingHolder::from_posting)
            .collect();
        TransactionHolder { trn, postings }
    }

    fn to_transaction(mut self) -> Transaction {
        self.trn.postings = self
            .postings
            .into_iter()
            .map(PostingHolder::to_posting)
            .collect();
        self.trn
    }

    fn all_postings_match_subset(&self, subset: &Transaction) -> bool {
        subset.postings.iter().all(|sub_posting| {
            self.postings
                .iter()
                .any(|sup_posting| sup_posting.matches(sub_posting))
        })
    }
}

/// Contains a partially unpacked `Posting`.
struct PostingHolder {
    posting: Posting,
    comment: Comment,
}

impl PostingHolder {
    fn from_posting(mut posting: Posting) -> Self {
        let comment = Comment::from_opt_comment(posting.comment.as_ref().map(String::as_str));
        posting.comment = None;
        PostingHolder { posting, comment }
    }

    fn to_posting(mut self) -> Posting {
        self.posting.comment = self.comment.to_opt_comment();
        self.posting
    }

    fn matches(&self, b: &Posting) -> bool {
        self.posting.account == b.account
            && self.posting.amount == b.amount
            && match (&self.posting.balance, &b.balance) {
                (Some(a_bal), Some(b_bal)) => a_bal == b_bal,
                _ => true,
            }
    }

    fn update(&mut self, src: &Posting) {
        // TODO: Merge/update status.
        if self.posting.balance.is_none() {
            self.posting.balance = src.balance.clone()
        }
        let src_comment = Comment::from_opt_comment(src.comment.as_ref().map(String::as_str));
        self.comment.merge_from(&src_comment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::parse_transactions;

    #[test]
    fn stable_sorts_destination_by_date() {
        let mut merger = Merger::new();
        merger.merge(parse_transactions(
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
        ));
        let result = &merger.build();
        assert_eq!(result.unmerged, vec![]);
        assert_transactions_eq!(
            &result.merged,
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

        merger.merge(parse_transactions(
            r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            "#,
        ));
        merger.merge(parse_transactions(
            r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00
                    income:salary    GBP -100.00
                2000/01/02 Lunch
                    assets:checking  GBP -5.00
                    expenses:dining  GBP 5.00
                "#,
        ));
        let result = &merger.build();
        assert_eq!(result.unmerged, vec![]);
        assert_transactions_eq!(
            &result.merged,
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

        merger.merge(parse_transactions(
            r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            "#,
        ));
        merger.merge(parse_transactions(
            r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00  =GBP 1234.00
                    income:salary    GBP -100.00
                "#,
        ));
        let result = &merger.build();
        assert_eq!(result.unmerged, vec![]);
        assert_transactions_eq!(
            &result.merged,
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

        merger.merge(parse_transactions(
            r#"
            2000/01/01 Salary
                assets:checking  GBP 100.00
                income:salary    GBP -100.00
            "#,
        ));
        merger.merge(parse_transactions(
            r#"
                2000/01/01 Salary
                    assets:checking  GBP 100.00  =GBP 1234.00
                    income:salary    GBP -100.00
                "#,
        ));
        let result = &merger.build();
        assert_eq!(result.unmerged, vec![]);
        assert_transactions_eq!(
            &result.merged,
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
    fn test_update_posting() {
        let parse_update = |dest: &str, src: &str| {
            let dest_posting = parse_posting(dest);
            let src_posting = parse_posting(src);
            let mut holder = PostingHolder::from_posting(dest_posting);
            holder.update(&src_posting);
            holder.to_posting()
        };
        assert_eq!(
            parse_update("foo  GBP 10.00", "foo  GBP 10.00 =GBP 90.00"),
            parse_posting("foo  GBP 10.00 =GBP 90.00"),
            "updates None balance",
        );
        assert_eq!(
            parse_update("foo  GBP 10.00 =GBP 50.00", "foo  GBP 10.00 =GBP 90.00"),
            parse_posting("foo  GBP 10.00 =GBP 50.00"),
            "does not update existing balance",
        );
        assert_eq!(
            parse_update(
                "foo  GBP 10.00 =GBP 50.00 ; key: old-value",
                "foo  GBP 10.00 =GBP 90.00 ; key: new-value"
            ),
            parse_posting("foo  GBP 10.00 =GBP 50.00 ; key: new-value"),
            "merges comments",
        );
    }
}
