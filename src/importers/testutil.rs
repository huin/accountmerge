use std::io::Write;

use goldenfile::Mint;

use crate::importers::importer::TransactionImporter;

pub fn golden_test(importer: &dyn TransactionImporter, golden_path: &str) {
    let mut mint = Mint::new("testdata/importers");
    let differ = Box::new(goldenfile::differs::text_diff);
    let mut out = mint
        .new_goldenfile_with_differ(golden_path, differ)
        .expect("new goldenfile");

    let ledger = ledger_parser::Ledger {
        transactions: importer.get_transactions().expect("perform import"),
        commodity_prices: Vec::new(),
    };

    let mut s = format!("{}", ledger);
    // Ensure that the file only ends in a single newline to make git
    // checks happy.
    while s.ends_with("\n\n") {
        s.pop();
    }

    out.write_all(s.as_bytes()).expect("write output");
}
