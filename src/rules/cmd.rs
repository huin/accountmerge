use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Error, Result};
use structopt::StructOpt;

use crate::filespec::{self, FileSpec};
use crate::internal::TransactionPostings;
use crate::rules::processor::TransactionProcessor;
use crate::rules::rhai::Rhai;
use crate::rules::table;

#[derive(Debug, StructOpt)]
pub struct Command {
    #[structopt(short = "r", long = "rules")]
    /// The file to read the rules from.
    rules: PathBuf,
    // The engine to interpret the rules as.
    #[structopt(short = "e", long = "engine")]
    engine: EngineSelection,
    /// The Ledger journal to read.
    input_journal: FileSpec,
    /// The ledger file to write to (overwrites any existing file). "-" writes
    /// to stdout.
    #[structopt(short = "o", long = "output", default_value = "-")]
    output: FileSpec,
}

#[derive(Clone, Copy, Debug, StructOpt)]
enum EngineSelection {
    Table,
    Rhai,
}

impl EngineSelection {
    fn build(self, source: &Path) -> Result<Box<dyn TransactionProcessor>> {
        match self {
            EngineSelection::Table => Ok(Box::new(table::load_from_path(source)?)),
            EngineSelection::Rhai => Ok(Box::new(Rhai::from_file(source)?)),
        }
    }
}

impl FromStr for EngineSelection {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use EngineSelection::*;
        match s {
            "table" => Ok(Table),
            "rhai" => Ok(Rhai),
            _ => {
                bail!("unknown engine: {}", s);
            }
        }
    }
}

impl Command {
    pub fn run(&self) -> Result<()> {
        let processor = self.engine.build(&self.rules)?;
        let mut ledger = filespec::read_ledger_file(&self.input_journal)?;
        let trns = TransactionPostings::take_from_ledger(&mut ledger);

        let new_trns = processor.update_transactions(trns)?;

        TransactionPostings::put_into_ledger(&mut ledger, new_trns);
        filespec::write_ledger_file(&self.output, &ledger)?;
        Ok(())
    }
}
