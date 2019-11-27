use std::fmt;

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

#[derive(Debug, Fail)]
enum ReadError {
    #[fail(display = "header not found in document")]
    HeaderNotFound {},
    #[fail(display = "bad value {} for {}", value, field)]
    BadValue { value: String, field: &'static str },
}

impl ReadError {
    fn bad_value<V: fmt::Debug>(value: V, field: &'static str) -> ReadError {
        ReadError::BadValue {
            value: format!("{:?}", value),
            field,
        }
    }
}

impl TransactionImporter for NationwidePdf {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        let doc = tesseract_tsv::Document::from_reader(self.input.reader()?)?;

        // Find the table and positions of its columns.
        let table: table::Table = table::Table::find_in_document(&doc)
            .ok_or_else(|| Error::from(ReadError::HeaderNotFound {}))?;

        let trn_lines = table.read_lines()?;
        for trn_line in trn_lines {
            eprintln!("Transaction line: {:?}", trn_line);
        }

        bail!("unimplemented")
    }
}

mod table {
    use chrono::format as date_fmt;
    use chrono::NaiveDate;
    use failure::Error;

    use super::ReadError;
    use crate::importers::tesseract_tsv::{self, Document, Paragraph, Word};

    const DATE: &str = "Date";
    const DETAILS: &str = "Details";
    const PAYMENTS: &str = "Payments";
    const RECEPITS: &str = "Receipts";
    const BALANCE: &str = "Balance";
    /// Earliest/latest years to accept from a PDF. These values are almost
    /// too forgiving, but should do as a sanity check.
    const EARLIEST_YEAR: i32 = 1980;
    const LATEST_YEAR: i32 = 2100;

    #[derive(Debug)]
    pub struct Table<'a> {
        columns: Columns,
        para: &'a Paragraph,
    }

    impl<'a> Table<'a> {
        pub fn find_in_document(doc: &'a Document) -> Option<Self> {
            doc.iter_paragraphs().find_map(|para| {
                Columns::find_in_paragraph(para).map(|columns| Table { columns, para })
            })
        }

        pub fn read_lines(&self) -> Result<Vec<TransactionLine>, Error> {
            let mut trn_lines = Vec::<TransactionLine>::new();
            let mut date_parts: chrono::format::Parsed = Default::default();
            let mut date: Option<NaiveDate> = None;
            // Skip first line that contains the header that we already found.
            for line in self.para.lines.iter().skip(1) {
                self.columns
                    .update_date_from_line(&mut date_parts, &mut date, line)?;
                trn_lines.push(TransactionLine {
                    implied_date: date,
                    detail: self.columns.details.join_words_in(line),
                    payment: self.columns.payments.join_words_in(line),
                    receipt: self.columns.receipts.join_words_in(line),
                    balance: self.columns.balance.join_words_in(line),
                });
            }
            Ok(trn_lines)
        }
    }

    #[derive(Debug)]
    struct Columns {
        date: ColumnPos,
        details: ColumnPos,
        payments: ColumnPos,
        receipts: ColumnPos,
        balance: ColumnPos,
    }

    impl Columns {
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
                date: ColumnPos {
                    // Date column contains values that extend left of the
                    // header a little. Use the "Date" header width as a fudge
                    // to include that.
                    left: line.words[0].left - line.words[0].width / 2,
                    right: line.words[1].left,
                },
                details: ColumnPos {
                    left: line.words[1].left,
                    right: line.words[2].left,
                },
                payments: ColumnPos {
                    left: line.words[2].left,
                    right: line.words[3].left,
                },
                receipts: ColumnPos {
                    left: line.words[3].left,
                    right: line.words[4].left,
                },
                balance: ColumnPos {
                    left: line.words[4].left,
                    // Similarly to "Date", the contents of the "Balance" column
                    // extend slightly to the right of the header itself. Use
                    // the "Balance" header width as a fudge to include that.
                    right: line.words[4].left + line.words[4].width * 2,
                },
            })
        }

        fn update_date_from_line(
            &self,
            date_parts: &mut date_fmt::Parsed,
            date: &mut Option<NaiveDate>,
            line: &tesseract_tsv::Line,
        ) -> Result<(), Error> {
            const DAY_PART: date_fmt::Item =
                date_fmt::Item::Numeric(date_fmt::Numeric::Day, date_fmt::Pad::Zero);
            const MONTH_PART: date_fmt::Item =
                date_fmt::Item::Fixed(date_fmt::Fixed::ShortMonthName);
            const YEAR_PART: date_fmt::Item =
                date_fmt::Item::Numeric(date_fmt::Numeric::Year, date_fmt::Pad::None);

            let date_words: Vec<&str> = self.date.collect_words_in(line).collect();
            match date_words.len() {
                0 => {
                    // No date information, this can be okay at this stage.
                    Ok(())
                }
                1 => {
                    date_parts.year = None;
                    parse_date_component(date_parts, YEAR_PART, date_words[0])?;
                    let new_year = date_parts.year.expect("year must be set");
                    if new_year < EARLIEST_YEAR || new_year > LATEST_YEAR {
                        return Err(ReadError::bad_value(new_year, "year").into());
                    }

                    // Changing the year typically implies that we are
                    // expecting a day and month on a following line, so
                    // invalidate those values.
                    date_parts.month = None;
                    date_parts.day = None;
                    Ok(())
                }
                2 => {
                    // Typically the dates comprised of two "words" are the
                    // 2-digit day-of-month, and 3-letter month. These
                    // combined with the year from a previous line provide a
                    // date.

                    // It seems to be necessary to clear these fields prior
                    // to parsing back into them.
                    date_parts.month = None;
                    date_parts.day = None;
                    parse_date_component(date_parts, DAY_PART, date_words[0])?;
                    parse_date_component(date_parts, MONTH_PART, date_words[1])?;
                    *date = Some(date_parts.to_naive_date()?);
                    Ok(())
                }
                _ => Err(ReadError::bad_value(date_parts, "date").into()),
            }
        }
    }

    fn parse_date_component(
        parsed: &mut date_fmt::Parsed,
        component: date_fmt::Item,
        value: &str,
    ) -> Result<(), Error> {
        let parts: [date_fmt::Item; 1] = [component];
        date_fmt::parse(parsed, value, parts.iter().cloned()).map_err(Into::into)
    }

    #[derive(Debug)]
    struct ColumnPos {
        left: i32,
        right: i32,
    }

    impl ColumnPos {
        fn join_words_in(&self, line: &tesseract_tsv::Line) -> Option<String> {
            let s = itertools::join(self.collect_words_in(line), " ");
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }

        fn collect_words_in<'a>(
            &'a self,
            line: &'a tesseract_tsv::Line,
        ) -> impl Iterator<Item = &'a str> + 'a {
            line.words
                .iter()
                .filter(move |word| self.contains_left(word))
                .map(|word| word.text.as_str())
        }

        fn contains_left(&self, word: &Word) -> bool {
            self.includes(word.left)
        }

        fn includes(&self, horizontal_point: i32) -> bool {
            self.left <= horizontal_point && horizontal_point < self.right
        }
    }

    /// Line containing some transaction information. It may not encompass
    /// complete information about the transaction, which may continue on
    /// following rows.
    #[derive(Debug)]
    pub struct TransactionLine {
        pub implied_date: Option<NaiveDate>,
        pub detail: Option<String>,
        pub payment: Option<String>,
        pub receipt: Option<String>,
        pub balance: Option<String>,
    }
}
