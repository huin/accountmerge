use ledger_parser::{Posting, Transaction};

pub struct Merger {
    transactions: Vec<Transaction>,
}

impl Merger {
    pub fn new() -> Self {
        Merger {
            transactions: Vec::new(),
        }
    }

    pub fn merge(&mut self, mut src: Vec<Transaction>) {
        src.sort_by(|a, b| a.date.cmp(&b.date));
        // TODO: Optimize transaction search based on date.
        // TODO: Search multiple matching transactions.
        for src_trn in src.into_iter() {
            if let Some(dest_trn) = self
                .transactions
                .iter_mut()
                .find(|dest_trn| transactions_match(*dest_trn, &src_trn))
            {
                for src_posting in &src_trn.postings {
                    if let Some(dest_posting) = dest_trn
                        .postings
                        .iter_mut()
                        .find(|dest_posting| postings_match(dest_posting, src_posting))
                    {
                        update_posting(dest_posting, src_posting);
                    }
                }
            } else {
                self.transactions.push(src_trn);
            }
        }
    }

    pub fn build(self) -> Vec<Transaction> {
        self.transactions
    }
}

fn transactions_match(a: &Transaction, b: &Transaction) -> bool {
    a.date == b.date && a.description == b.description && all_transaction_postings_match(a, b)
}

fn all_transaction_postings_match(a: &Transaction, b: &Transaction) -> bool {
    for pa in &a.postings {
        if b.postings.iter().any(|pb| postings_match(pa, pb)) {
            continue;
        } else {
            return false;
        }
    }
    true
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
