use ledger_parser::Transaction;

pub fn parse_transactions(s: &str) -> Vec<Transaction> {
    ledger_parser::parse(textwrap::dedent(s).as_ref())
        .expect("test input did not parse")
        .transactions
}

pub fn format_transactions(transactions: &Vec<Transaction>) -> String {
    let mut result = String::new();
    for trn in transactions {
        result.push_str(&format!("{}", trn));
    }
    result
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
