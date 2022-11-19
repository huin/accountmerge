//! Helpers for handling ledger-parser structures.

use ledger_parser::{Amount, Ledger, LedgerItem, PostingAmount, Transaction};

/// Returns a `PostingAmount` with only `.amount` set.
pub fn simple_posting_amount(amount: Amount) -> PostingAmount {
    PostingAmount {
        amount,
        lot_price: None,
        price: None,
    }
}

pub fn ledger_from_transactions(transactions: Vec<Transaction>) -> Ledger {
    Ledger {
        items: transactions
            .into_iter()
            .map(LedgerItem::Transaction)
            .collect(),
    }
}
