use std::ffi::OsStr;
use std::path::PathBuf;
use std::str::FromStr;

use chrono::NaiveDate;
use failure::{Error, ResultExt};
use ledger_parser::{Amount, Posting, Transaction};
use regex::Regex;
use rust_decimal::Decimal;
use structopt::StructOpt;

use crate::accounts;
use crate::comment::Comment;
use crate::fingerprint::{make_prefix, FingerprintBuilder};
use crate::importers::importer::TransactionImporter;
use crate::importers::nationwide::{CommonOpts, BANK_NAME};
use crate::importers::tesseract;
use crate::importers::util;
use crate::tags;

#[derive(Debug, StructOpt)]
/// Converts from Nationwide (nationwide.co.uk) PDF statements to Ledger
/// transactions. It assumes that Graphics Magick and Tesseract v4 executables
/// are installed.
pub struct NationwidePdf {
    /// PDF file to read.
    input: PathBuf,
    /// Path to Graphics Magick binary to run.
    #[structopt(default_value = "gm")]
    graphics_magic_binary: PathBuf,
    /// Path to Tesseract v4 binary to run.
    #[structopt(default_value = "tesseract")]
    tesseract_binary: PathBuf,

    #[structopt(flatten)]
    commonopts: CommonOpts,
}

#[derive(Debug, Fail)]
enum ReadError {
    #[fail(display = "general error: {}", reason)]
    General { reason: String },
    #[fail(display = "bad input structure: {}", reason)]
    Structure { reason: String },
}

impl ReadError {
    fn general<S: Into<String>>(reason: S) -> ReadError {
        ReadError::General {
            reason: reason.into(),
        }
    }
    fn structure<S: Into<String>>(reason: S) -> ReadError {
        ReadError::Structure {
            reason: reason.into(),
        }
    }
}

impl TransactionImporter for NationwidePdf {
    fn get_transactions(&self) -> Result<Vec<Transaction>, Error> {
        let doc = self.ocr_document().context("OCR scanning PDF")?;

        let account_name = find_account_name(&doc)
            .ok_or_else(|| Error::from(ReadError::structure("account name not found")))?;

        let fp_prefix = make_prefix(&self.commonopts.fp_prefix.to_prefix(&account_name));

        let mut acc = TransactionsAccumulator::new(fp_prefix.to_string());
        for page in &doc.pages {
            for table in table::Table::find_in_page(page) {
                let trn_lines = table.read_lines().with_context(|_| {
                    format!(
                        "failed to read transaction lines from table on page #{}",
                        page.num
                    )
                })?;
                self.lines_to_transactions(&mut acc, trn_lines)
                    .with_context(|_| {
                        format!("failed to process transaction lines on page #{}", page.num)
                    })?;
            }
        }

        Ok(acc.build())
    }
}

impl NationwidePdf {
    /// Performs OCR on the PDF file, extracting a `Document`.
    fn ocr_document(&self) -> Result<tesseract::Document, Error> {
        use std::fs::File;
        use std::process::Command;

        let tmpdir =
            temporary::Directory::new("nationwide-pdf").context("creating temporary directory")?;
        let png_pattern = tmpdir.path().join("page-*.png");
        let png_pattern_str = png_pattern
            .to_str()
            .ok_or_else(|| ReadError::general("converting glob path to utf-8 string"))?;

        {
            let png_fmt = tmpdir.path().join("page-%02d.png");
            let gm_args: [&OsStr; 6] = [
                "convert".as_ref(),
                // DPI of the PNG files.
                "-density".as_ref(),
                "300".as_ref(),
                self.input.as_os_str(),
                // Output a PNG file per page in the PDF, according to png_fmt.
                "+adjoin".as_ref(),
                png_fmt.as_os_str(),
            ];

            Command::new(self.graphics_magic_binary.as_os_str())
                .args(&gm_args)
                .status()
                .context("converting PDF into PNG files")?;
        }

        let png_list_file_path = tmpdir.path().join("png-files.txt");
        {
            use std::io::Write;
            let mut png_list_file =
                File::create(&png_list_file_path).context("creating file to list PNG files")?;
            let png_glob = glob::glob(png_pattern_str).context("globbing for PNG files")?;
            for png_path_result in png_glob {
                let png_path = png_path_result?;
                let png_path_str = png_path.to_str().ok_or_else(|| {
                    ReadError::general("converting PNG file path to utf-8 string")
                })?;
                png_list_file.write_all(png_path_str.as_bytes())?;
                png_list_file.write_all("\n".as_bytes())?;
            }
        }

        let output_base = tmpdir.path().join("ocr");
        {
            let tess_args: [&OsStr; 7] = [
                // Language model to use (English).
                "-l".as_ref(),
                "eng".as_ref(),
                // DPI of the PNG files.
                "--dpi".as_ref(),
                "300".as_ref(),
                // Text file containing PNG filenames, which treats them each as
                // a page of input in the OCR output.
                png_list_file_path.as_os_str(),
                // Base filename for the TSV output file.
                output_base.as_os_str(),
                // Configuration to use (i.e output format).
                "tsv".as_ref(),
            ];
            Command::new(self.tesseract_binary.as_os_str())
                .args(&tess_args)
                .status()
                .context("performing OCR on PNG files")?;
        }

        {
            let output_path = output_base.with_extension("tsv");
            let tsv_file = File::open(&output_path)
                .with_context(|_| format!("opening TSV output file {:?}", output_path))?;
            tesseract::Document::from_tsv_reader(tsv_file)
        }
    }

    fn lines_to_transactions(
        &self,
        acc: &mut TransactionsAccumulator,
        trn_lines: Vec<table::TransactionLine>,
    ) -> Result<(), Error> {
        let mut prev_trn_line: Option<&table::TransactionLine> = None;

        for trn_line in &trn_lines {
            if let Some(prev_trn_line) = prev_trn_line {
                if trn_line.top > prev_trn_line.top + prev_trn_line.height * 2 {
                    // There have been blank lines, this is probably the end of
                    // the transactions.
                    break;
                }
            }

            acc.feed_line(trn_line)
                .with_context(|_| format!("for transaction line {}", trn_line))?;

            prev_trn_line = Some(trn_line);
        }

        Ok(())
    }
}

struct TransactionsAccumulator {
    fp_prefix: String,
    cur_trn_opt: Option<TransactionBuilder>,
    prev_date: Option<NaiveDate>,
    date_counter: i32,
    trns: Vec<Transaction>,
}

impl TransactionsAccumulator {
    fn new(fp_prefix: String) -> Self {
        Self {
            fp_prefix,
            cur_trn_opt: None,
            prev_date: None,
            date_counter: 0,
            trns: Vec::new(),
        }
    }

    fn feed_line(&mut self, trn_line: &table::TransactionLine) -> Result<(), Error> {
        match (&trn_line.payment, &trn_line.receipt) {
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
                self.flush_transaction();
                if trn_line.implied_date != self.prev_date {
                    self.date_counter = 0;
                } else {
                    self.date_counter += 1;
                }
                self.cur_trn_opt = Some(TransactionBuilder::new(
                    trn_line.implied_date,
                    self.date_counter,
                    parse_amount(&payment)?,
                    TransactionType::Payment,
                    trn_line.detail.clone(),
                )?);
            }
            (None, Some(receipt)) => {
                // Start of new receipt transaction.
                self.flush_transaction();
                if trn_line.implied_date != self.prev_date {
                    self.date_counter = 0;
                } else {
                    self.date_counter += 1;
                }
                self.cur_trn_opt = Some(TransactionBuilder::new(
                    trn_line.implied_date,
                    self.date_counter,
                    parse_amount(&receipt)?,
                    TransactionType::Receipt,
                    trn_line.detail.clone(),
                )?);
            }
            (None, None) => {
                // Continuation of prior transaction.
                // Use this to amend cur_trn_opt.
                let cur_trn: &mut TransactionBuilder = if let Some(cur_trn) = &mut self.cur_trn_opt
                {
                    cur_trn
                } else {
                    return Ok(());
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

        self.prev_date = trn_line.implied_date;

        let cur_trn: &mut TransactionBuilder = if let Some(cur_trn) = &mut self.cur_trn_opt {
            cur_trn
        } else {
            return Ok(());
        };

        if let Some(balance) = &trn_line.balance {
            cur_trn.balance = Some(parse_amount(balance)?);
        }

        Ok(())
    }

    fn flush_transaction(&mut self) {
        if let Some(pending) = self.cur_trn_opt.take() {
            self.trns.push(pending.build(&self.fp_prefix));
        }
    }

    fn build(mut self) -> Vec<Transaction> {
        self.flush_transaction();
        self.trns
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
            .with_value_tag(tags::BANK, BANK_NAME)
            .with_tag(tags::UNKNOWN_ACCOUNT);

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
                    amount: Some(halves.self_.amount),
                    balance: self.balance.map(ledger_parser::Balance::Amount),
                    status: None,
                    comment: comment_base
                        .clone()
                        .with_tag(tags::IMPORT_SELF)
                        .with_tag(self_fp.build_with_prefix(fp_prefix))
                        .build()
                        .into_opt_comment(),
                },
                Posting {
                    account: halves.peer.account,
                    amount: Some(halves.peer.amount),
                    balance: None,
                    status: None,
                    comment: comment_base
                        .with_tag(tags::IMPORT_PEER)
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
    use std::fmt;

    use chrono::format as date_fmt;
    use chrono::NaiveDate;
    use failure::Error;

    use super::ReadError;
    use crate::importers::tesseract::{self, Line, Page, Paragraph, Word};

    const DATE: &str = "Date";
    const DETAILS: &str = "Details";
    const PAYMENTS: &str = "Payments";
    const RECEPITS: &str = "Receipts";
    const BALANCE: &str = "Balance";
    /// Earliest/latest years to accept from a PDF. These values are almost
    /// too forgiving, but should do as a sanity check.
    const EARLIEST_YEAR: i32 = 1980;
    const LATEST_YEAR: i32 = 2100;

    pub struct Table<'a> {
        columns: Columns,
        para: &'a Paragraph,
    }

    impl<'a> Table<'a> {
        pub fn find_in_page(page: &'a Page) -> impl Iterator<Item = Table<'a>> + 'a {
            page.blocks
                .iter()
                .flat_map(|block| block.paragraphs.iter())
                .filter_map(|para| {
                    Columns::find_in_paragraph(para).map(|columns| Table { columns, para })
                })
        }

        pub fn read_lines(&self) -> Result<Vec<TransactionLine>, Error> {
            let mut trn_lines = Vec::<TransactionLine>::new();
            let mut date_parts: chrono::format::Parsed = Default::default();
            let mut date: Option<NaiveDate> = None;
            // Skip lines up to and including the line that contains the header
            // that we already found.
            for line in self
                .para
                .lines
                .iter()
                .skip(self.columns.header_line_idx + 1)
            {
                match self
                    .columns
                    .update_date_from_line(&mut date_parts, &mut date, line)?
                {
                    DateField::Year => {
                        // A transaction will not start on this line.
                        // Lines starting with years only specify the year, and
                        // maybe a carry-over balance.
                    }
                    _ => {
                        // Lines that start with day and month or nothing at all
                        // can be part of a transaction.
                        if let Some(detail) = self.columns.details.join_words_in(line) {
                            trn_lines.push(TransactionLine {
                                implied_date: date,
                                detail,
                                payment: self.columns.payments.join_words_in(line),
                                receipt: self.columns.receipts.join_words_in(line),
                                balance: self.columns.balance.join_words_in(line),
                                top: line.top,
                                height: line.height,
                            });
                        }
                    }
                }
            }
            Ok(trn_lines)
        }
    }

    impl<'a> fmt::Debug for Table<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(
                f,
                "Table with columns {:?} in paragraph #{}",
                self.columns, self.para.num
            )
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

        // Spatial position of the line on the page.
        pub top: i32,
        pub height: i32,
    }

    impl fmt::Display for TransactionLine {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            if let Some(implied_date) = &self.implied_date {
                write!(f, "{}", implied_date)?;
            }
            write!(f, " {}", self.detail)?;
            if let Some(payment) = &self.payment {
                write!(f, " {}", payment)?;
            }
            if let Some(receipt) = &self.receipt {
                write!(f, " {}", receipt)?;
            }
            if let Some(balance) = &self.balance {
                write!(f, " {}", balance)?;
            }

            Ok(())
        }
    }

    #[derive(Debug)]
    struct Columns {
        header_line_idx: usize,
        date: ColumnPos,
        details: ColumnPos,
        payments: ColumnPos,
        receipts: ColumnPos,
        balance: ColumnPos,
    }

    impl Columns {
        fn find_in_paragraph(paragraph: &Paragraph) -> Option<Self> {
            for (line_idx, line) in paragraph.lines.iter().enumerate() {
                if let Some(columns) = Self::find_in_line(line_idx, line) {
                    return Some(columns);
                }
            }
            None
        }

        fn find_in_line(line_idx: usize, line: &Line) -> Option<Self> {
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
                header_line_idx: line_idx,
                date: ColumnPos {
                    horiz_bounds: line.words[0].horiz_bounds(),
                },
                details: ColumnPos::new(line.words[1].left, line.words[2].left),
                payments: ColumnPos::new(line.words[2].left, line.words[3].left),
                receipts: ColumnPos::new(line.words[3].left, line.words[4].left),
                balance: ColumnPos {
                    horiz_bounds: line.words[4].horiz_bounds(),
                },
            })
        }

        fn update_date_from_line(
            &self,
            date_parts: &mut date_fmt::Parsed,
            date: &mut Option<NaiveDate>,
            line: &tesseract::Line,
        ) -> Result<DateField, Error> {
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
                    Ok(DateField::Nothing)
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
                    Ok(DateField::Year)
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
                    Ok(DateField::DayMonth)
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
        horiz_bounds: tesseract::Bounds,
    }

    impl ColumnPos {
        fn new(left: i32, right: i32) -> Self {
            Self {
                horiz_bounds: tesseract::Bounds {
                    min: left,
                    max: right,
                },
            }
        }

        fn join_words_in(&self, line: &tesseract::Line) -> Option<String> {
            let s = itertools::join(self.collect_words_in(line), " ");
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }

        fn collect_words_in<'a>(
            &'a self,
            line: &'a tesseract::Line,
        ) -> impl Iterator<Item = &'a str> + 'a {
            line.words
                .iter()
                .filter(move |word| self.overlaps(word))
                .map(|word| word.text.as_str())
        }

        fn overlaps(&self, word: &Word) -> bool {
            self.horiz_bounds.overlaps(word.horiz_bounds())
        }
    }

    enum DateField {
        Nothing,
        Year,
        DayMonth,
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
fn find_account_name(doc: &tesseract::Document) -> Option<String> {
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
