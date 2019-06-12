use chrono::naive::NaiveDate;
use ledger_parser::{Amount, Posting, Transaction};

pub struct TransactionBuilder {
    trn: Transaction,
}

impl TransactionBuilder {
    pub fn new<S: Into<String>>(date: NaiveDate, description: S) -> Self {
        TransactionBuilder {
            trn: Transaction {
                code: None,
                status: None,
                comment: None,
                effective_date: None,
                date,
                description: description.into(),
                postings: vec![],
            },
        }
    }

    pub fn posting<S: Into<String>>(
        mut self,
        account: S,
        amount: Amount,
        balance: Option<Amount>,
    ) -> Self {
        self.trn.postings.push(Posting {
            status: None,
            account: account.into(),
            amount,
            balance,
            comment: None,
        });
        self
    }

    pub fn build(self) -> Transaction {
        self.trn
    }
}
