use failure::Error;
use ledger_parser::Transaction;
use structopt::StructOpt;

use crate::filespec::FileSpec;
use crate::importers::importer::TransactionImporter;
use crate::importers::tesseract_tsv;
use crate::importers::tesseract_tsv::Paragraph;

#[derive(Debug, StructOpt)]
/// Converts from Nationwide (nationwide.co.uk) PDF statements to Ledger
/// transactions. It (currently) relies on external processes to have performed
/// some OCR to Tesseract TSV files.
pub struct NationwidePdf {
    /// Tesseract TSV output file to read. "-" reads from stdin.
    input: FileSpec,
}

#[derive(Debug, Fail)]
enum ReadError {
    #[fail(display = "header not found in document")]
    HeaderNotFound {},
}

impl TransactionImporter for NationwidePdf {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        let doc = tesseract_tsv::Document::from_reader(self.input.reader()?)?;

        doc.debug_write_to(Box::new(std::io::stderr()))?;

        // Find header line and the positions of the headers.
        let (header_positions, table_para): (table::Headers, &Paragraph) =
            table::Headers::find_in_document(&doc)
                .ok_or_else(|| Error::from(ReadError::HeaderNotFound {}))?;

        eprintln!(
            "Found headers at {:?} in paragraph {}",
            header_positions, table_para.num
        );

        bail!("unimplemented")
    }
}

mod table {
    use crate::importers::tesseract_tsv::{Document, Paragraph};

    const DATE: &str = "Date";
    const DETAILS: &str = "Details";
    const PAYMENTS: &str = "Payments";
    const RECEPITS: &str = "Receipts";
    const BALANCE: &str = "Balance";

    #[derive(Debug)]
    pub struct Headers {
        pub date: i32,
        pub details: i32,
        pub payments: i32,
        pub receipts: i32,
        pub balance: i32,
    }

    impl Headers {
        pub fn find_in_document(doc: &Document) -> Option<(Self, &Paragraph)> {
            doc.iter_paragraphs().find_map(|paragraph| {
                Self::find_in_paragraph(paragraph).map(|positions| (positions, paragraph))
            })
        }

        fn find_in_paragraph(paragraph: &Paragraph) -> Option<Self> {
            let line = paragraph.lines.get(0)?;
            if line.words.len() < 5 {
                return None;
            }

            if line.words[0].text != DATE
                || line.words[1].text != DETAILS
                || line.words[2].text != PAYMENTS
                || line.words[3].text != RECEPITS
                || line.words[4].text != BALANCE
            {
                return None;
            }

            Some(Self {
                date: line.words[0].left,
                details: line.words[1].left,
                payments: line.words[2].left,
                receipts: line.words[3].left,
                balance: line.words[4].left,
            })
        }
    }
}
