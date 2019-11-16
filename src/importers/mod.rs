use failure::Error;
use ledger_parser::Ledger;
use structopt::StructOpt;

mod importer;
mod nationwide_csv;
mod paypal_csv;
mod util;

#[cfg(test)]
mod testutil;

use importer::TransactionImporter;

#[derive(Debug, StructOpt)]
pub enum Importer {
    #[structopt(name = "nationwide-csv")]
    /// Converts from Nationwide (nationwide.co.uk) CSV format to Ledger
    /// transactions.
    NationwideCsv(nationwide_csv::NationwideCsv),
    #[structopt(name = "paypal-csv")]
    /// Converts from PayPal CSV format to Ledger transactions.
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
