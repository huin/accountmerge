use anyhow::Result;
use clap::{Args, Subcommand};
use ledger_parser::Ledger;

use crate::filespec::{self, FileSpec};
use crate::importers;
use crate::importers::importer::TransactionImporter;
use crate::ledgerutil::ledger_from_transactions;

#[derive(Debug, Subcommand)]
pub enum Importer {
    /// Converts from Nationwide (nationwide.co.uk) CSV format to Ledger
    /// transactions.
    #[command(name = "nationwide-csv")]
    NationwideCsv(importers::nationwide_csv::NationwideCsv),
    /// Converts from Nationwide (nationwide.co.uk) PDF format to Ledger
    /// transactions.
    #[command(name = "nationwide-pdf")]
    NationwidePdf(importers::nationwide_pdf::NationwidePdf),
    /// Converts from PayPal CSV format to Ledger transactions.
    #[command(name = "paypal-csv")]
    PaypalCsv(importers::paypal_csv::PaypalCsv),
}

impl Importer {
    pub fn do_import(&self) -> Result<Ledger> {
        let transactions = self.get_importer().get_transactions()?;
        Ok(ledger_from_transactions(transactions))
    }

    fn get_importer(&self) -> &dyn TransactionImporter {
        use Importer::*;
        match self {
            NationwideCsv(imp) => imp,
            NationwidePdf(imp) => imp,
            PaypalCsv(imp) => imp,
        }
    }
}

#[derive(Debug, Args)]
pub struct Command {
    /// The ledger file to write to (overwrites any existing file). "-" writes
    /// to stdout.
    #[arg(short = 'o', long = "output", default_value = "-")]
    output: FileSpec,
    /// The importer type to use to read transactions.
    #[command(subcommand)]
    importer: Importer,
}

impl Command {
    pub fn run(&self) -> Result<()> {
        let ledger = self.importer.do_import()?;
        filespec::write_ledger_file(&self.output, &ledger)
    }
}
