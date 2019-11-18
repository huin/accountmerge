use failure::Error;
use structopt::StructOpt;

mod table;

use crate::filespec::{self, FileSpec};

#[derive(Debug, StructOpt)]
pub struct Command {
    #[structopt(short = "r", long = "rules")]
    /// The file to read the rules from.
    rules: FileSpec,
    /// The Ledger journal to read.
    input_journal: FileSpec,
    /// The ledger file to write to (overwrites any existing file). "-" writes
    /// to stdout.
    #[structopt(short = "o", long = "output", default_value = "-")]
    output: FileSpec,
}

impl Command {
    pub fn run(&self) -> Result<(), Error> {
        let mut ledger = filespec::read_ledger_file(&self.input_journal)?;
        let rules = table::Table::from_filespec(&self.rules)?;
        for trn in &mut ledger.transactions {
            rules.update_transaction(trn)?;
        }
        filespec::write_ledger_file(&self.output, &ledger)
    }
}
