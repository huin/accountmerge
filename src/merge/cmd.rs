use failure::Error;
use structopt::StructOpt;

use crate::filespec::{self, FileSpec};
use crate::internal::TransactionPostings;
use crate::merge::{merger, sources};

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
            for trns in sources::read_ledger_file(ledger_file)? {
                let mut unmerged_trns = merger.merge(trns)?;
                unmerged.append(&mut unmerged_trns.0);
            }
        }

        if !unmerged.is_empty() {
            match self.unmerged.as_ref() {
                Some(fs) => {
                    let mut ledger = ledger_parser::Ledger {
                        commodity_prices: Default::default(),
                        transactions: Default::default(),
                    };
                    // Deliberately leave the source tags on the unmerged files
                    // so that:
                    // * The human has more context of where the transaction
                    //   came from.
                    // * When re-attempting to merge from the unmerged file, the
                    //   sources::read_ledger_file can cause each source in the
                    //   file to be merged independently.
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

        let mut trns = merger.build();
        sources::strip_sources(&mut trns);
        let mut ledger = ledger_parser::Ledger {
            commodity_prices: Default::default(),
            transactions: Default::default(),
        };
        TransactionPostings::put_into_ledger(&mut ledger, trns);

        filespec::write_ledger_file(&self.output, &ledger)
    }
}
