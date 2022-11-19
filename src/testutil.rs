use ledger_parser::{LedgerItem, Posting, Transaction};

use crate::comment::Comment;
use crate::internal::{PostingInternal, TransactionPostings};

pub fn parse_transaction_postings(s: &str) -> Vec<TransactionPostings> {
    let ledger =
        ledger_parser::parse(textwrap::dedent(s).as_ref()).expect("test input did not parse");
    TransactionPostings::from_ledger(ledger).expect("expected success")
}

pub fn format_transaction_postings(transactions: Vec<TransactionPostings>) -> String {
    let mut result = String::new();
    for trn in transactions {
        let raw_trn: Transaction = trn.into();
        result.push_str(&format!("{}", raw_trn));
    }
    result
}

pub fn normalize_comment(text: &mut Option<String>) {
    let c = Comment::from_opt_comment(text.as_ref().map(String::as_str));
    *text = c.into_opt_comment();
}

#[macro_export]
macro_rules! assert_transactions_eq {
    ($want:expr, $got:expr, $($context_arg:expr),*) => {
        let want_str = crate::testutil::format_transactions(&$want);
        let got_str = crate::testutil::format_transactions(&$got);
        if want_str != got_str {
            eprintln!($($context_arg,)*);
            text_diff::assert_diff(&want_str, &got_str, "\n", 0);
        }
    };
    ($want:expr, $got:expr) => {
        let want_str = crate::testutil::format_transactions(&$want);
        let got_str = crate::testutil::format_transactions(&$got);
        if want_str != got_str {
            text_diff::assert_diff(&want_str, &got_str, "\n", 0);
        }
    };
}

#[macro_export]
macro_rules! assert_transaction_postings_eq {
    ($want:expr, $got:expr, $($context_arg:expr),*) => {
        let want_str = crate::testutil::format_transaction_postings($want);
        let got_str = crate::testutil::format_transaction_postings($got);
        if want_str != got_str {
            eprintln!($($context_arg,)*);
            text_diff::assert_diff(&want_str, &got_str, "\n", 0);
        }
    };
    ($want:expr, $got:expr) => {
        let want_str = crate::testutil::format_transaction_postings($want);
        let got_str = crate::testutil::format_transaction_postings($got);
        if want_str != got_str {
            text_diff::assert_diff(&want_str, &got_str, "\n", 0);
        }
    };
}

pub fn parse_posting(p: &str) -> Posting {
    let t = "2000/01/01 Dummy Transaction\n  ".to_string() + p + "\n";
    let mut ledger = ledger_parser::parse(&t).unwrap();
    let mut trn = match ledger.items.remove(0) {
        LedgerItem::Transaction(trn) => trn,
        other => panic!("got {:?}, want transaction", other),
    };
    let mut post = trn.postings.remove(0);
    normalize_comment(&mut post.comment);
    post
}

pub fn parse_posting_internal(p: &str) -> PostingInternal {
    parse_posting(p).into()
}

pub fn format_posting_internal(post: PostingInternal) -> String {
    let raw_post: Posting = post.into();
    format!("{}", raw_post)
}

#[macro_export]
macro_rules! assert_posting_internal_eq {
    ($want:expr, $got:expr, $($context_arg:expr),*) => {
        let want_str = crate::testutil::format_posting_internal($want);
        let got_str = crate::testutil::format_posting_internal($got);
        if want_str != got_str {
            eprintln!($($context_arg,)*);
            text_diff::assert_diff(&want_str, &got_str, "\n", 0);
        }
    };
    ($want:expr, $got:expr) => {
        let want_str = crate::testutil::format_posting_internal($want);
        let got_str = crate::testutil::format_posting_internal($got);
        if want_str != got_str {
            text_diff::assert_diff(&want_str, &got_str, "\n", 0);
        }
    };
}
