//! Reads Tesseract OCR TSV files into a hierarchical structure for further
//! processing.

use failure::Error;

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

#[derive(Debug)]
pub struct Document {
    pages: Vec<Page>,
}

impl Document {
    fn new() -> Self {
        Self { pages: Vec::new() }
    }

    pub fn from_reader<R: std::io::Read>(reader: R) -> Result<Self, Error> {
        let r = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .has_headers(true)
            .trim(csv::Trim::All)
            .from_reader(reader);

        let mut doc = Document::new();

        for record_res in r.into_deserialize() {
            let record: Record = record_res?;
            doc.feed_record(record)?;
        }

        Ok(doc)
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

    pub fn debug_write_to(&self, mut w: Box<dyn std::io::Write>) -> Result<(), Error> {
        writeln!(w, "Document")?;
        for page in &self.pages {
            writeln!(w, "  Page #{}", page.num)?;
            for block in &page.blocks {
                writeln!(w, "    Block #{}", block.num)?;
                for para in &block.paragraphs {
                    writeln!(w, "      Paragraph #{}", para.num)?;
                    for line in &para.lines {
                        writeln!(w, "        Line #{}", line.num)?;
                        writeln!(
                            w,
                            "          {}",
                            itertools::join(
                                line.words
                                    .iter()
                                    .map(|word| format!("{}(l:{})", &word.text, word.left)),
                                " "
                            ),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Page {
    pub num: i32,
    pub blocks: Vec<Block>,
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
    pub num: i32,
    pub paragraphs: Vec<Paragraph>,
}

impl From<Record> for Block {
    fn from(record: Record) -> Self {
        Self {
            num: record.block_num,
            paragraphs: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct Paragraph {
    pub num: i32,
    pub lines: Vec<Line>,
}

impl From<Record> for Paragraph {
    fn from(record: Record) -> Self {
        Self {
            num: record.par_num,
            lines: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct Line {
    pub num: i32,
    pub words: Vec<Word>,
}

impl From<Record> for Line {
    fn from(record: Record) -> Self {
        Self {
            num: record.line_num,
            words: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct Word {
    pub num: i32,
    pub left: i32,
    pub width: i32,
    pub text: String,
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
