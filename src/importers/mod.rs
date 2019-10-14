use std::path::PathBuf;

use failure::Error;
use ledger_parser::{Ledger, Transaction};
use structopt::StructOpt;

mod nationwide_csv;

#[derive(Debug, StructOpt)]
pub enum Importer {
    #[structopt(name = "nationwide-csv")]
    NationwideCsv(NationwideCsv),
}

impl Importer {
    pub fn do_import(&self) -> Result<Ledger, Error> {
        Ok(Ledger {
            transactions: self.get_transactions()?,
            commodity_prices: Default::default(),
        })
    }

    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        use Importer::*;
        match self {
            NationwideCsv(imp) => imp.get_transactions(),
        }
    }
}

#[derive(Debug, StructOpt)]
pub struct NationwideCsv {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
}

impl NationwideCsv {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        nationwide_csv::transactions_from_path(&self.input)
    }
}
