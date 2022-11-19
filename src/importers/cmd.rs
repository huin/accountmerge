use anyhow::{anyhow, bail, Result};
use clap::{Args, Subcommand};

use crate::filespec::{self, FileSpec};
use crate::importers;
use crate::importers::importer::TransactionImporter;
use crate::ledgerutil::ledger_from_transactions;

use super::importer::Import;

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
    pub fn do_import(&self) -> Result<Import> {
        self.get_importer().get_transactions()
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
    /// If true then perform the following substitution in the --output path:
    ///
    /// "%FP_NS%" -> replaced with the user provided fingerprint namespace.
    #[arg(long = "sub-output-path", default_value_t = false)]
    substitute_output_path: bool,
    /// If true, then create any parent directories of the file in --output  (if they don't alredy
    /// exist).
    #[arg(long = "make-parent-dirs", default_value_t = false)]
    make_parent_dirs: bool,
    /// The importer type to use to read transactions.
    #[command(subcommand)]
    importer: Importer,
}

impl Command {
    pub fn run(&self) -> Result<()> {
        let import = self.importer.do_import()?;
        let output = if !self.substitute_output_path {
            self.output.clone()
        } else {
            let p = match self.output {
                FileSpec::Stdio => {
                    bail!("--sub-output-path only works with file paths, not stdout")
                }
                FileSpec::Path(ref p) => p,
            };
            let p_str = p.to_str().ok_or_else(|| {
                anyhow!("--sub-output-path only works if --output is a UTF-8 path")
            })?;
            let new_p = p_str.replace("%FP_NS%", &import.user_fp_namespace);
            FileSpec::Path(new_p.into())
        };

        if self.make_parent_dirs {
            match output {
                FileSpec::Stdio => {
                    bail!("--make-parent-dirs only works with file paths, not stdout")
                }
                FileSpec::Path(ref p) => {
                    if let Some(parent) = p.parent() {
                        std::fs::create_dir_all(parent).map_err(anyhow::Error::from)?;
                    }
                }
            }
        }

        let ledger = ledger_from_transactions(import.transactions);
        filespec::write_ledger_file(&output, &ledger)
    }
}
