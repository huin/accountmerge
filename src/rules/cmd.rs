use anyhow::Result;
use clap::{Args, Subcommand};

use crate::filespec::{self, FileSpec};
use crate::internal::TransactionPostings;
use crate::rules::processor::TransactionProcessorFactory;

#[derive(Debug, Args)]
pub struct Command {
    // The engine to interpret the rules as.
    #[command(subcommand)]
    engine: Engine,
    /// The Ledger journal to read.
    input_journal: FileSpec,
    /// The ledger file to write to (overwrites any existing file). "-" writes
    /// to stdout.
    #[arg(short = 'o', long = "output", default_value = "-")]
    output: FileSpec,
}

#[derive(Debug, Subcommand)]
enum Engine {
    #[command(name = "rhai")]
    Rhai(crate::rules::rhai::Command),
    #[command(name = "table")]
    Table(crate::rules::table::Command),
}

impl Engine {
    fn get_factory(&self) -> &dyn TransactionProcessorFactory {
        use Engine::*;
        match self {
            Rhai(cmd) => cmd,
            Table(cmd) => cmd,
        }
    }
}

impl Command {
    pub fn run(&self) -> Result<()> {
        let processor = self.engine.get_factory().make_processor()?;
        let mut ledger = filespec::read_ledger_file(&self.input_journal)?;
        let trns = TransactionPostings::take_from_ledger(&mut ledger);

        let new_trns = processor.update_transactions(trns)?;

        TransactionPostings::put_into_ledger(&mut ledger, new_trns);
        filespec::write_ledger_file(&self.output, &ledger)?;
        Ok(())
    }
}
