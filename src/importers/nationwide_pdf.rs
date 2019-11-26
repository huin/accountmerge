use failure::Error;
use ledger_parser::Transaction;
use structopt::StructOpt;

use crate::filespec::FileSpec;
use crate::importers::importer::TransactionImporter;
use crate::importers::tesseract_tsv;

#[derive(Debug, StructOpt)]
/// Converts from Nationwide (nationwide.co.uk) PDF statements to Ledger
/// transactions. It (currently) relies on external processes to have performed
/// some OCR to Tesseract TSV files.
pub struct NationwidePdf {
    /// Tesseract TSV output file to read. "-" reads from stdin.
    input: FileSpec,
}

impl TransactionImporter for NationwidePdf {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        let doc = tesseract_tsv::Document::from_reader(self.input.reader()?)?;
        eprintln!("Document: {:?}", doc);

        bail!("unimplemented")
    }
}
