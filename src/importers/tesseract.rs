//! Reads Tesseract OCR TSV files into a hierarchical structure for further
//! processing.

use anyhow::Result;

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
    // Confidence value currently unused.
    #[allow(dead_code)]
    conf: i32,
    text: String,
}

#[derive(Debug)]
pub struct Document {
    pub pages: Vec<Page>,
}

impl Document {
    fn new() -> Self {
        Self { pages: Vec::new() }
    }

    /// Reads the `Document` from a reader of a Tesseract TSV file.
    pub fn from_tsv_reader<R: std::io::Read>(reader: R) -> Result<Self> {
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

    fn feed_record(&mut self, record: Record) -> Result<()> {
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
                bail!("TSV field level has bad TSV value {:?}", record.level);
            }
        }

        Ok(())
    }

    fn page_mut(&mut self, record: &Record) -> Result<&mut Page> {
        get_checked_mut(&mut self.pages, record.page_num, "page_num")
    }

    fn block_mut(&mut self, record: &Record) -> Result<&mut Block> {
        self.page_mut(record)
            .and_then(|page| get_checked_mut(&mut page.blocks, record.block_num, "block_num"))
    }

    fn paragraph_mut(&mut self, record: &Record) -> Result<&mut Paragraph> {
        self.block_mut(record)
            .and_then(|block| get_checked_mut(&mut block.paragraphs, record.par_num, "par_num"))
    }

    fn line_mut(&mut self, record: &Record) -> Result<&mut Line> {
        self.paragraph_mut(record).and_then(|paragraph| {
            get_checked_mut(&mut paragraph.lines, record.line_num, "line_num")
        })
    }

    /// Helper function for use when working out the structure output by
    /// Tesseract OCR.
    #[allow(dead_code)]
    pub fn debug_write_to(&self, mut w: Box<dyn std::io::Write>) -> Result<()> {
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
                                line.words.iter().map(|word| format!(
                                    "{}({}-{})",
                                    &word.text,
                                    word.left,
                                    word.left + word.width
                                )),
                                " "
                            ),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn iter_paragraphs(&self) -> impl Iterator<Item = &Paragraph> {
        self.pages
            .iter()
            .flat_map(|page| page.blocks.iter())
            .flat_map(|block| block.paragraphs.iter())
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
    pub top: i32,
    pub height: i32,
    pub words: Vec<Word>,
}

impl From<Record> for Line {
    fn from(record: Record) -> Self {
        Self {
            num: record.line_num,
            top: record.top,
            height: record.height,
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

impl Word {
    pub fn horiz_bounds(&self) -> Bounds {
        Bounds {
            min: self.left,
            max: self.left + self.width,
        }
    }
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

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min: i32,
    pub max: i32,
}

impl Bounds {
    pub fn overlaps(self, other: Self) -> bool {
        self.max > other.min && self.min < other.max
    }
}

fn get_checked_mut<'a, T>(
    v: &'a mut [T],
    num: i32,
    num_field: &'static str,
) -> Result<&'a mut T> {
    let idx = num_to_idx(num, num_field)?;
    v.get_mut(idx)
        .ok_or_else(|| anyhow!("TSV field {} has bad TSV value {:?}", num_field, num))
}

fn push_checked<T>(
    v: &mut Vec<T>,
    num: i32,
    num_field: &'static str,
    record: Record,
    type_: &'static str,
    parent_type: &'static str,
) -> Result<()>
where
    T: From<Record>,
{
    let idx = num_to_idx(num, num_field)?;
    if idx != v.len() {
        bail!("TSV {} is missing its parent {}", type_, parent_type);
    }
    v.push(record.into());
    Ok(())
}

fn num_to_idx(num: i32, num_field: &'static str) -> Result<usize> {
    if num < 1 {
        bail!("TSV field {} has bad TSV value {:?}", num_field, num);
    }
    Ok(num as usize - 1)
}
