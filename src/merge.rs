use std::collections::HashMap;

use chrono::NaiveDate;
use ledger_parser::{Posting, Transaction};

pub struct Merger {
    trn_arena: Vec<Transaction>,
    trn_by_date: HashMap<NaiveDate, Vec<usize>>,
}

const EMPTY_INDICES: [usize; 0] = [];

impl Merger {
    pub fn new() -> Self {
        Merger {
            trn_arena: Vec::new(),
            trn_by_date: HashMap::new(),
        }
    }

    pub fn merge(&mut self, src: Vec<Transaction>) {
        // Reuse vector allocation in loop (cleared each time).
        let mut candidate_trns = Vec::<usize>::new();

        for src_trn in src.into_iter() {
            candidate_trns.clear();

            // Find multiple matching transactions.
            candidate_trns.extend(self.iter_trns_for_date(&src_trn.date).filter(|&index| {
                all_transaction_postings_match_subset(&self.trn_arena[index], &src_trn)
            }));

            if candidate_trns.len() == 1 {
                let dest_trn = &mut self.trn_arena[candidate_trns[0]];
                for src_posting in &src_trn.postings {
                    if let Some(dest_posting) = dest_trn
                        .postings
                        .iter_mut()
                        .find(|dest_posting| postings_match(dest_posting, src_posting))
                    {
                        update_posting(dest_posting, src_posting);
                    }
                }
            } else if candidate_trns.len() > 1 {
                unimplemented!("TODO - how to handle when multiple transactions match the input");
            } else {
                self.add_transaction(src_trn);
            }
        }
    }

    fn iter_trns_for_date<'a>(&'a self, date: &NaiveDate) -> impl Iterator<Item = usize> + 'a {
        self.trn_by_date
            .get(date)
            .map_or_else(|| EMPTY_INDICES.as_ref(), |id_vec| id_vec.as_slice())
            .iter()
            .map(|&index| index)
    }

    fn add_transaction(&mut self, trn: Transaction) -> usize {
        let date = trn.date;
        self.trn_arena.push(trn);
        let index = self.trn_arena.len() - 1;
        self.trn_by_date
            .entry(date)
            .or_insert(Vec::new())
            .push(index);
        index
    }

    pub fn build(self) -> Vec<Transaction> {
        let mut dates: Vec<NaiveDate> = self.trn_by_date.keys().cloned().collect();
        dates.sort();
        let mut trn_by_date = self.trn_by_date;
        let mut trn_arena: Vec<Option<Transaction>> =
            self.trn_arena.into_iter().map(|trn| Some(trn)).collect();
        let mut out = Vec::<Transaction>::new();
        for date in &dates {
            if let Some(date_trn_indices) = trn_by_date.remove(date) {
                for index in date_trn_indices {
                    let mut trn: Option<Transaction> = None;
                    std::mem::swap(&mut trn, &mut trn_arena[index]);
                    out.push(trn.expect("duplicate index in date_trn_indices"));
                }
            }
        }
        out
    }
}

fn all_transaction_postings_match_subset(superset: &Transaction, subset: &Transaction) -> bool {
    subset.postings.iter().all(|sub_posting| {
        superset
            .postings
            .iter()
            .any(|sup_posting| postings_match(sub_posting, sup_posting))
    })
}

fn postings_match(a: &Posting, b: &Posting) -> bool {
    a.account == b.account
        && a.amount == b.amount
        && match (&a.balance, &b.balance) {
            (Some(a_bal), Some(b_bal)) => a_bal == b_bal,
            _ => true,
        }
}

fn update_posting(dest: &mut Posting, src: &Posting) {
    // TODO: Merge/update comments.
    // TODO: Merge/update status.
    if dest.balance.is_none() {
        dest.balance = src.balance.clone()
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
        assert_transactions_eq!(
            &merger.build(),
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
        assert_transactions_eq!(
            merger.build(),
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
        assert_transactions_eq!(
            &merger.build(),
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
        assert_transactions_eq!(
            &merger.build(),
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
            let mut dest_posting = parse_posting(dest);
            let src_posting = parse_posting(src);
            update_posting(&mut dest_posting, &src_posting);
            dest_posting
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
    }
}
