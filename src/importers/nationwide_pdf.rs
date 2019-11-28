use std::str::FromStr;

use chrono::NaiveDate;
use failure::Error;
use ledger_parser::{Amount, Posting, Transaction};
use regex::Regex;
use rust_decimal::Decimal;
use structopt::StructOpt;

use crate::accounts;
use crate::comment::Comment;
use crate::filespec::FileSpec;
use crate::fingerprint::{make_prefix, FingerprintBuilder};
use crate::importers::importer::TransactionImporter;
use crate::importers::nationwide::{FpPrefix, BANK_NAME};
use crate::importers::tesseract_tsv;
use crate::importers::util;
use crate::tags;

#[derive(Debug, StructOpt)]
/// Converts from Nationwide (nationwide.co.uk) PDF statements to Ledger
/// transactions. It (currently) relies on external processes to have performed
/// some OCR to Tesseract TSV files.
pub struct NationwidePdf {
    /// Tesseract TSV output file to read. "-" reads from stdin.
    input: FileSpec,

    /// The prefix of the fingerprints to generate (without "fp-" that will be
    /// prefixed to this value).
    ///
    /// "account-name" uses the account name from the CSV file.
    ///
    /// "fixed:<prefix>" uses the given fixed prefix.
    ///
    /// "generated" generates a hashed value based on the account name in the
    /// CSV file.
    #[structopt(long = "fingerprint-prefix", default_value = "generated")]
    fp_prefix: FpPrefix,
}

#[derive(Debug, Fail)]
enum ReadError {
    #[fail(display = "bad input structure: {}", reason)]
    Structure { reason: String },
}

impl ReadError {
    fn structure<S: Into<String>>(reason: S) -> ReadError {
        ReadError::Structure {
            reason: reason.into(),
        }
    }
}

impl TransactionImporter for NationwidePdf {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        let doc = tesseract_tsv::Document::from_reader(self.input.reader()?)?;

        let account_name = find_account_name(&doc)
            .ok_or_else(|| Error::from(ReadError::structure("account name not found")))?;

        let fp_prefix = make_prefix(&self.fp_prefix.to_prefix(&account_name));

        // Find the table and positions of its columns.
        let table: table::Table = table::Table::find_in_document(&doc).ok_or_else(|| {
            Error::from(ReadError::structure("transaction table header not found"))
        })?;

        let trn_lines = table.read_lines()?;

        self.lines_to_transactions(trn_lines, &fp_prefix)
    }
}

impl NationwidePdf {
    fn lines_to_transactions(
        &self,
        trn_lines: Vec<table::TransactionLine>,
        fp_prefix: &str,
    ) -> Result<Vec<Transaction>, Error> {
        let mut trns = Vec::<Transaction>::new();
        let mut cur_trn_opt: Option<TransactionBuilder> = None;
        let mut prev_date: Option<NaiveDate> = None;
        let mut date_counter = 0i32;
        for trn_line in trn_lines {
            match (trn_line.payment, trn_line.receipt) {
                (Some(payment), Some(receipt)) => {
                    // Should not happen.
                    return Err(ReadError::structure(format!(
                        "transaction line has values for both payment ({:?}) and receipt ({:?})",
                        payment, receipt,
                    ))
                    .into());
                }
                (Some(payment), None) => {
                    // Start of new payment transaction.
                    flush_transaction(&mut trns, &mut cur_trn_opt, fp_prefix);
                    if trn_line.implied_date != prev_date {
                        date_counter = 0;
                    } else {
                        date_counter += 1;
                    }
                    cur_trn_opt = Some(TransactionBuilder::new(
                        trn_line.implied_date,
                        date_counter,
                        parse_amount(&payment)?,
                        TransactionType::Payment,
                        trn_line.detail,
                    )?);
                }
                (None, Some(receipt)) => {
                    // Start of new receipt transaction.
                    flush_transaction(&mut trns, &mut cur_trn_opt, fp_prefix);
                    if trn_line.implied_date != prev_date {
                        date_counter = 0;
                    } else {
                        date_counter += 1;
                    }
                    cur_trn_opt = Some(TransactionBuilder::new(
                        trn_line.implied_date,
                        date_counter,
                        parse_amount(&receipt)?,
                        TransactionType::Receipt,
                        trn_line.detail,
                    )?);
                }
                (None, None) => {
                    // Continuation of prior transaction.
                    // Use this to amend cur_trn_opt.
                    let cur_trn: &mut TransactionBuilder = if let Some(cur_trn) = &mut cur_trn_opt {
                        cur_trn
                    } else {
                        continue;
                    };

                    if trn_line.detail.is_empty() {
                        // No use for empty detail
                    } else if trn_line.detail.starts_with("Effective Date ") {
                        let edate =
                            NaiveDate::parse_from_str(&trn_line.detail, "Effective Date %d %b %Y")?;
                        cur_trn.effective_date = Some(edate);
                    } else {
                        cur_trn.description.push_str(" ");
                        cur_trn.description.push_str(&trn_line.detail);
                    }
                }
            }

            prev_date = trn_line.implied_date;

            let cur_trn: &mut TransactionBuilder = if let Some(cur_trn) = &mut cur_trn_opt {
                cur_trn
            } else {
                continue;
            };

            if let Some(balance) = trn_line.balance {
                cur_trn.balance = Some(parse_amount(&balance)?);
            }
        }

        flush_transaction(&mut trns, &mut cur_trn_opt, fp_prefix);
        Ok(trns)
    }
}

fn flush_transaction(
    trns: &mut Vec<Transaction>,
    opt_builder: &mut Option<TransactionBuilder>,
    fp_prefix: &str,
) {
    if let Some(pending) = opt_builder.take() {
        trns.push(pending.build(fp_prefix));
    }
}

struct TransactionBuilder {
    date: NaiveDate,
    date_counter: i32,
    amount: Amount,
    type_: TransactionType,
    effective_date: Option<NaiveDate>,
    description: String,
    balance: Option<Amount>,
}

enum TransactionType {
    Payment,
    Receipt,
}

impl TransactionBuilder {
    fn new(
        date: Option<NaiveDate>,
        date_counter: i32,
        amount: Amount,
        type_: TransactionType,
        description: String,
    ) -> Result<Self, Error> {
        let date = date.ok_or_else(|| {
            ReadError::structure(format!("missing date for transaction {:?}", description))
        })?;
        if amount.quantity.is_sign_negative() {
            // Negative values should not appear on input, income vs expense is
            // signalled via the "Payments" vs "Receipts" columns.
            return Err(ReadError::structure(format!(
                "encountered negative amount for payment or receipt: {}",
                amount.quantity
            ))
            .into());
        }

        Ok(TransactionBuilder {
            date,
            date_counter,
            amount,
            type_,
            effective_date: None,
            description,
            balance: None,
        })
    }

    fn build(self, fp_prefix: &str) -> Transaction {
        let record_fpb = FingerprintBuilder::new()
            .with(self.date)
            .with(self.date_counter)
            .with(self.description.as_str());

        let halves = util::self_and_peer_account_amount(
            match self.type_ {
                TransactionType::Payment => util::negate_amount(self.amount),
                TransactionType::Receipt => self.amount,
            },
            accounts::ASSETS_UNKNOWN.to_string(),
        );
        let comment_base = Comment::builder()
            .with_value_tag(tags::BANK_TAG, BANK_NAME)
            .with_tag(tags::UNKNOWN_ACCOUNT_TAG);

        let self_fp = record_fpb
            .clone()
            .with(halves.self_.account.as_str())
            .with(&halves.self_.amount);
        let peer_fp = record_fpb
            .with(halves.peer.account.as_str())
            .with(&halves.peer.amount);

        Transaction {
            date: self.date,
            effective_date: self.effective_date,
            status: None,
            code: None,
            description: self.description,
            comment: None,
            postings: vec![
                Posting {
                    account: halves.self_.account,
                    amount: halves.self_.amount,
                    balance: self.balance.map(ledger_parser::Balance::Amount),
                    status: None,
                    comment: comment_base
                        .clone()
                        .with_tag(tags::IMPORT_SELF_TAG)
                        .with_tag(self_fp.build_with_prefix(fp_prefix))
                        .build()
                        .into_opt_comment(),
                },
                Posting {
                    account: halves.peer.account,
                    amount: halves.peer.amount,
                    balance: None,
                    status: None,
                    comment: comment_base
                        .with_tag(tags::IMPORT_PEER_TAG)
                        .with_tag(peer_fp.build_with_prefix(fp_prefix))
                        .build()
                        .into_opt_comment(),
                },
            ],
        }
    }
}

fn parse_amount(s: &str) -> Result<Amount, Error> {
    let quantity = if s.contains(',') {
        Decimal::from_str(&s.replace(",", ""))?
    } else {
        Decimal::from_str(s)?
    };
    Ok(Amount {
        quantity,
        commodity: ledger_parser::Commodity {
            name: "GBP".to_string(),
            position: ledger_parser::CommodityPosition::Left,
        },
    })
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
                let detail = self.columns.details.join_words_in(line).ok_or_else(|| {
                    Error::from(ReadError::structure("missing detail for transaction"))
                })?;
                trn_lines.push(TransactionLine {
                    implied_date: date,
                    detail,
                    payment: self.columns.payments.join_words_in(line),
                    receipt: self.columns.receipts.join_words_in(line),
                    balance: self.columns.balance.join_words_in(line),
                });
            }
            Ok(trn_lines)
        }
    }

    /// Line containing some transaction information. It may not encompass
    /// complete information about the transaction, which may continue on
    /// following rows.
    #[derive(Debug)]
    pub struct TransactionLine {
        pub implied_date: Option<NaiveDate>,
        pub detail: String,
        pub payment: Option<String>,
        pub receipt: Option<String>,
        pub balance: Option<String>,
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
                        return Err(ReadError::structure(format!(
                            "found year {} which is out of the expected range",
                            new_year
                        ))
                        .into());
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
                _ => Err(ReadError::structure(format!(
                    "date had unexpected set of components: {}",
                    date_words.join(" ")
                ))
                .into()),
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
}

/// Looks for a line starting with text like:
///
/// ```
/// Account Number 12-34-56 12345678
/// ```
///
/// ... and returns the sort code and account number as a string (separated by a
/// space).
fn find_account_name(doc: &tesseract_tsv::Document) -> Option<String> {
    lazy_static! {
        static ref SORT_CODE_RX: Regex = Regex::new(r"^\d{2}-\d{2}-\d{2}$").unwrap();
    }
    lazy_static! {
        static ref ACCT_NUM_RX: Regex = Regex::new(r"^\d{8}$").unwrap();
    }

    for para in doc.iter_paragraphs() {
        for line in &para.lines {
            if line.words.len() < 4 {
                continue;
            }
            let word_account = &line.words[0].text;
            let word_number = &line.words[1].text;
            let sort_code = &line.words[2].text;
            let acct_num = &line.words[3].text;
            if word_account != "Account" || word_number != "Number" {
                continue;
            }
            if !SORT_CODE_RX.is_match(sort_code) {
                continue;
            }
            if !ACCT_NUM_RX.is_match(acct_num) {
                continue;
            }

            return Some(format!("{} {}", sort_code, acct_num));
        }
    }

    None
}
