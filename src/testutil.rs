use ledger_parser::{Posting, Transaction};

use crate::comment::Comment;
use crate::internal::{PostingInternal, TransactionPostings};

pub fn parse_transactions(s: &str) -> Vec<Transaction> {
    let mut trns = ledger_parser::parse(textwrap::dedent(s).as_ref())
        .expect("test input did not parse")
        .transactions;
    // Reformat comments to normalize the format used in tests.
    for trn in &mut trns {
        for post in &mut trn.postings {
            normalize_comment(&mut post.comment);
        }
    }
    trns
}

pub fn parse_transaction_postings(s: &str) -> Vec<TransactionPostings> {
    let trns = ledger_parser::parse(textwrap::dedent(s).as_ref())
        .expect("test input did not parse")
        .transactions;
    trns.into_iter().map(Into::into).collect()
}

pub fn format_transactions(transactions: &Vec<Transaction>) -> String {
    let mut result = String::new();
    for trn in transactions {
        result.push_str(&format!("{}", trn));
    }
    result
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
    let mut trn = ledger_parser::parse(&t).unwrap();
    let mut post = trn.transactions.remove(0).postings.remove(0);
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
