use std::path::PathBuf;

use chrono_tz::Tz;
use failure::Error;
use ledger_parser::{Ledger, Transaction};
use structopt::StructOpt;

mod importer;
mod nationwide_csv;
mod paypal_csv;
mod util;

use importer::TransactionImporter;

#[derive(Debug, StructOpt)]
pub enum Importer {
    #[structopt(name = "nationwide-csv")]
    NationwideCsv(NationwideCsv),
    #[structopt(name = "paypal-csv")]
    PaypalCsv(PaypalCsv),
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
pub struct NationwideCsv {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
}

impl TransactionImporter for NationwideCsv {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        nationwide_csv::transactions_from_path(&self.input)
    }
}

#[derive(Debug, StructOpt)]
pub struct PaypalCsv {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
    output_timezone: Tz,
}

impl TransactionImporter for PaypalCsv {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        paypal_csv::transactions_from_path(&self.input, &self.output_timezone)
    }
}
