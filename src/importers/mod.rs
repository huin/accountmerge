use failure::Error;
use ledger_parser::Ledger;
use structopt::StructOpt;

mod importer;
mod nationwide_csv;
mod paypal_csv;
mod util;

#[cfg(test)]
mod testutil;

use crate::filespec::{self, FileSpec};
use importer::TransactionImporter;

#[derive(Debug, StructOpt)]
pub enum Importer {
    /// Converts from Nationwide (nationwide.co.uk) CSV format to Ledger
    /// transactions.
    #[structopt(name = "nationwide-csv")]
    NationwideCsv(nationwide_csv::NationwideCsv),
    /// Converts from PayPal CSV format to Ledger transactions.
    #[structopt(name = "paypal-csv")]
    PaypalCsv(paypal_csv::PaypalCsv),
}

impl Importer {
    pub fn do_import(&self) -> Result<Ledger, Error> {
        let transactions = self.get_importer().get_transactions()?;
        Ok(Ledger {
            transactions,
            commodity_prices: Default::default(),
        })
    }

    fn get_importer(&self) -> &dyn TransactionImporter {
        use Importer::*;
        match self {
            NationwideCsv(imp) => imp,
            PaypalCsv(imp) => imp,
        }
    }
}

#[derive(Debug, StructOpt)]
pub struct Command {
    /// The ledger file to write to (overwrites any existing file). "-" writes
    /// to stdout.
    #[structopt(short = "o", long = "output", default_value = "-")]
    output: FileSpec,
    #[structopt(subcommand)]
    /// The importer type to use to read transactions.
    importer: Importer,
}

impl Command {
    pub fn run(&self) -> Result<(), Error> {
        let ledger = self.importer.do_import()?;
        filespec::write_ledger_file(&self.output, &ledger)
    }
}
