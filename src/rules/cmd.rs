use std::path::PathBuf;

use anyhow::Result;
use structopt::StructOpt;

use crate::filespec::{self, FileSpec};
use crate::internal::TransactionPostings;
use crate::rules::table;

#[derive(Debug, StructOpt)]
pub struct Command {
    #[structopt(short = "r", long = "rules")]
    /// The file to read the rules from.
    rules: PathBuf,
    /// The Ledger journal to read.
    input_journal: FileSpec,
    /// The ledger file to write to (overwrites any existing file). "-" writes
    /// to stdout.
    #[structopt(short = "o", long = "output", default_value = "-")]
    output: FileSpec,
}

impl Command {
    pub fn run(&self) -> Result<()> {
        let rules = table::load_from_path(&self.rules)?;
        let mut ledger = filespec::read_ledger_file(&self.input_journal)?;
        let trns = TransactionPostings::take_from_ledger(&mut ledger);

        let new_trns = rules.update_transactions(trns)?;

        TransactionPostings::put_into_ledger(&mut ledger, new_trns);
        filespec::write_ledger_file(&self.output, &ledger)?;
        Ok(())
    }
}
