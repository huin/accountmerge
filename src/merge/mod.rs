use failure::Error;
use structopt::StructOpt;

mod matchset;
pub mod merger;
mod posting;
mod transaction;

use crate::filespec::{self, FileSpec};
use crate::internal::TransactionPostings;

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(display = "bad input to merge: {}", reason)]
    Input { reason: String },
    #[fail(display = "internal merge error: {}", reason)]
    Internal { reason: String },
}

#[derive(Debug, StructOpt)]
pub struct Command {
    /// The Ledger journals to read from.
    inputs: Vec<FileSpec>,

    /// The file to write any unmerged transactions into.
    #[structopt(short = "u", long = "unmerged")]
    unmerged: Option<FileSpec>,

    /// The file to write the merged ledger to.
    #[structopt(short = "o", long = "output", default_value = "-")]
    output: FileSpec,
}

impl Command {
    pub fn run(&self) -> Result<(), Error> {
        let mut merger = merger::Merger::new();

        let mut unmerged = Vec::<TransactionPostings>::new();

        for ledger_file in &self.inputs {
            let mut ledger = filespec::read_ledger_file(ledger_file)?;
            let trns = TransactionPostings::take_from_ledger(&mut ledger);
            let mut unmerged_trns = merger.merge(trns)?;

            // TODO: Need to be able to differentiate between where the files where
            // the unmerged transaction originally came from. Tagging?
            unmerged.append(&mut unmerged_trns.0);
        }

        if !unmerged.is_empty() {
            match self.unmerged.as_ref() {
                Some(fs) => {
                    let mut ledger = ledger_parser::Ledger {
                        commodity_prices: Default::default(),
                        transactions: Default::default(),
                    };
                    TransactionPostings::put_into_ledger(&mut ledger, unmerged);
                    filespec::write_ledger_file(fs, &ledger)?;
                }
                None => {
                    return Err(format_err!(
                    "{} input transactions have gone unmerged and no --unmerged output file was specified",
                    unmerged.len(),
                ));
                }
            }
        }

        let mut ledger = ledger_parser::Ledger {
            commodity_prices: Default::default(),
            transactions: Default::default(),
        };
        TransactionPostings::put_into_ledger(&mut ledger, merger.build());

        filespec::write_ledger_file(&self.output, &ledger)
    }
}
