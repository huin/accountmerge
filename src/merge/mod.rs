use failure::Error;
use structopt::StructOpt;

mod matchset;
pub mod merger;
mod posting;
mod transaction;

use crate::filespec::{self, FileSpec};

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(display = "bad input to merge: {}", reason)]
    Input { reason: String },
}

#[derive(Debug, StructOpt)]
pub struct Merge {
    /// The Ledger journals to read from.
    inputs: Vec<FileSpec>,

    /// The file to write any unmerged transactions into.
    #[structopt(short = "u", long = "unmerged")]
    unmerged: Option<FileSpec>,

    /// The file to write the merged ledger to.
    #[structopt(short = "o", long = "output", default_value = "-")]
    output: FileSpec,
}

pub fn do_merge(merge: &Merge) -> Result<(), Error> {
    let mut merger = merger::Merger::new();

    let mut unmerged = ledger_parser::Ledger {
        commodity_prices: Default::default(),
        transactions: Default::default(),
    };

    for ledger_file in &merge.inputs {
        let ledger = filespec::read_ledger_file(ledger_file)?;
        let mut unmerged_trns = merger.merge(ledger.transactions)?;

        // TODO: Need to be able to differentiate between where the files where
        // the unmerged transaction originally came from. Tagging?
        unmerged.transactions.append(&mut unmerged_trns.0);
    }

    if !unmerged.transactions.is_empty() {
        match merge.unmerged.as_ref() {
            Some(fs) => {
                filespec::write_ledger_file(fs, &unmerged)?;
            }
            None => {
                return Err(format_err!(
                    "{} input transactions have gone unmerged and no --unmerged output file was specified",
                    unmerged.transactions.len(),
                ));
            }
        }
    }

    let transactions = merger.build();

    let ledger = ledger_parser::Ledger {
        transactions,
        commodity_prices: Default::default(),
    };

    filespec::write_ledger_file(&merge.output, &ledger)
}
