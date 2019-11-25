use failure::Error;
use ledger_parser::Transaction;
use structopt::StructOpt;

use crate::filespec::FileSpec;
use crate::importers::importer::TransactionImporter;

#[derive(Debug, Fail)]
enum ReadError {
    #[fail(display = "TSV field {} has bad TSV value {:?}", field, value)]
    TsvField { field: &'static str, value: i32 },
    #[fail(display = "TSV {} is missing its parent {}", type_, parent_type)]
    TsvParent {
        type_: &'static str,
        parent_type: &'static str,
    },
}

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
        let r = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .has_headers(true)
            .trim(csv::Trim::All)
            .from_reader(self.input.reader()?);

        let doc = de::to_document(r.into_deserialize())?;
        eprintln!("Document: {:?}", doc);

        bail!("unimplemented")
    }
}

mod de {
    use failure::Error;

    use super::ReadError;

    /// A Tesseract TSV file record.
    #[derive(Debug, Deserialize)]
    pub struct Record {
        level: i32,
        page_num: i32,
        block_num: i32,
        par_num: i32,
        line_num: i32,
        word_num: i32,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        conf: i32,
        text: String,
    }

    pub fn to_document<R>(
        records: csv::DeserializeRecordsIntoIter<R, Record>,
    ) -> Result<Document, Error>
    where
        R: std::io::Read,
    {
        let mut doc = Document::new();

        for record_res in records {
            let record: Record = record_res?;
            doc.feed_record(record)?;
        }

        Ok(doc)
    }

    #[derive(Debug)]
    pub struct Document {
        pages: Vec<Page>,
    }

    impl Document {
        fn new() -> Self {
            Self { pages: Vec::new() }
        }

        fn feed_record(&mut self, record: Record) -> Result<(), Error> {
            match record.level {
                1 => {
                    // New page.
                    push_checked(
                        &mut self.pages,
                        record.page_num,
                        "page_num",
                        record,
                        "page",
                        "document",
                    )?;
                }
                2 => {
                    // New block.
                    let page = self.page_mut(&record)?;
                    push_checked(
                        &mut page.blocks,
                        record.block_num,
                        "block_num",
                        record,
                        "block",
                        "page",
                    )?;
                }
                3 => {
                    // New paragraph.
                    let block = self.block_mut(&record)?;
                    push_checked(
                        &mut block.paragraphs,
                        record.par_num,
                        "par_num",
                        record,
                        "paragraph",
                        "block",
                    )?;
                }
                4 => {
                    // New line.
                    let paragraph = self.paragraph_mut(&record)?;
                    push_checked(
                        &mut paragraph.lines,
                        record.line_num,
                        "line_num",
                        record,
                        "line",
                        "paragraph",
                    )?;
                }
                5 => {
                    // New word.
                    let line = self.line_mut(&record)?;
                    push_checked(
                        &mut line.words,
                        record.word_num,
                        "word_num",
                        record,
                        "word",
                        "line",
                    )?;
                }
                _ => {
                    return Err(ReadError::TsvField {
                        field: "level",
                        value: record.level,
                    }
                    .into());
                }
            }

            Ok(())
        }

        fn page_mut(&mut self, record: &Record) -> Result<&mut Page, Error> {
            get_checked_mut(&mut self.pages, record.page_num, "page_num")
        }

        fn block_mut(&mut self, record: &Record) -> Result<&mut Block, Error> {
            self.page_mut(record)
                .and_then(|page| get_checked_mut(&mut page.blocks, record.block_num, "block_num"))
        }

        fn paragraph_mut(&mut self, record: &Record) -> Result<&mut Paragraph, Error> {
            self.block_mut(record)
                .and_then(|block| get_checked_mut(&mut block.paragraphs, record.par_num, "par_num"))
        }

        fn line_mut(&mut self, record: &Record) -> Result<&mut Line, Error> {
            self.paragraph_mut(record).and_then(|paragraph| {
                get_checked_mut(&mut paragraph.lines, record.line_num, "line_num")
            })
        }
    }

    #[derive(Debug)]
    pub struct Page {
        num: i32,
        blocks: Vec<Block>,
    }

    impl From<Record> for Page {
        fn from(record: Record) -> Self {
            Self {
                num: record.page_num,
                blocks: Vec::new(),
            }
        }
    }

    #[derive(Debug)]
    pub struct Block {
        num: i32,
        paragraphs: Vec<Paragraph>,
    }

    impl From<Record> for Block {
        fn from(record: Record) -> Self {
            Self {
                num: record.page_num,
                paragraphs: Vec::new(),
            }
        }
    }

    #[derive(Debug)]
    pub struct Paragraph {
        num: i32,
        lines: Vec<Line>,
    }

    impl From<Record> for Paragraph {
        fn from(record: Record) -> Self {
            Self {
                num: record.page_num,
                lines: Vec::new(),
            }
        }
    }

    #[derive(Debug)]
    pub struct Line {
        num: i32,
        words: Vec<Word>,
    }

    impl From<Record> for Line {
        fn from(record: Record) -> Self {
            Self {
                num: record.page_num,
                words: Vec::new(),
            }
        }
    }

    #[derive(Debug)]
    pub struct Word {
        num: i32,
        left: i32,
        width: i32,
        text: String,
    }

    impl From<Record> for Word {
        fn from(record: Record) -> Self {
            Self {
                num: record.word_num,
                left: record.left,
                width: record.width,
                text: record.text,
            }
        }
    }

    fn get_checked_mut<'a, T>(
        v: &'a mut Vec<T>,
        num: i32,
        num_field: &'static str,
    ) -> Result<&'a mut T, Error> {
        let idx = num_to_idx(num, num_field)?;
        v.get_mut(idx).ok_or_else(|| {
            ReadError::TsvField {
                field: num_field,
                value: num,
            }
            .into()
        })
    }

    fn push_checked<T>(
        v: &mut Vec<T>,
        num: i32,
        num_field: &'static str,
        record: Record,
        type_: &'static str,
        parent_type: &'static str,
    ) -> Result<(), Error>
    where
        T: From<Record>,
    {
        let idx = num_to_idx(num, num_field)?;
        if idx != v.len() {
            return Err(ReadError::TsvParent { type_, parent_type }.into());
        }
        v.push(record.into());
        Ok(())
    }

    fn num_to_idx(num: i32, num_field: &'static str) -> Result<usize, Error> {
        if num < 1 {
            Err(ReadError::TsvField {
                field: num_field,
                value: num,
            }
            .into())
        } else {
            Ok(num as usize - 1)
        }
    }
}
