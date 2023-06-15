use anyhow::Result;
use clap::Args;

use crate::filespec::{self, FileSpec};

#[derive(Debug, Args)]
pub struct Cmd {
    /// The Ledger journals to format.
    journals: Vec<FileSpec>,
}

impl Cmd {
    pub fn run(&self) -> Result<()> {
        for ledger_file in &self.journals {
            let ledger = filespec::read_ledger_file(ledger_file)?;
            filespec::write_ledger_file(ledger_file, &ledger)?;
        }

        Ok(())
    }
}
