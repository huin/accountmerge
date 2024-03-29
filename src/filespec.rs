//! Functions to read and write text files. Allows use of "-" as a way to
//! specify stdin or stdout.

use std::fmt;
use std::fs::File;
use std::io::{stdin, stdout, Read, Write};
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Error, Result};
use ledger_parser::Ledger;

/// Specifies a file to read from to write to (depending on context).
#[derive(Clone, Debug)]
pub enum FileSpec {
    /// Read from stdin or write to stdout.
    Stdio,
    /// Read from or write to the file at the given path.
    Path(PathBuf),
}

impl fmt::Display for FileSpec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use FileSpec::*;
        match self {
            Stdio => f.write_str("<stdio>"),
            Path(path) => write!(f, "{:?}", path),
        }
    }
}

impl FileSpec {
    pub fn reader(&self) -> Result<Box<dyn Read>> {
        use FileSpec::*;
        Ok(match self {
            Stdio => Box::new(stdin()),
            Path(path) => Box::new(
                File::open(path).with_context(|| format!("opening {:?} for reading", path))?,
            ),
        })
    }

    pub fn writer(&self) -> Result<Box<dyn Write>> {
        use FileSpec::*;
        Ok(match self {
            Stdio => Box::new(stdout()),
            Path(path) => Box::new(
                File::create(path).with_context(|| format!("opening {:?} for writing", path))?,
            ),
        })
    }
}

impl FromStr for FileSpec {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        use FileSpec::*;
        if s == "-" {
            Ok(Stdio)
        } else {
            Ok(Path(s.into()))
        }
    }
}

pub fn read_file(file_spec: &FileSpec) -> Result<String> {
    let mut f = file_spec.reader()?;
    let mut content = String::new();
    f.read_to_string(&mut content)?;
    Ok(content)
}

pub fn read_ledger_file(file_spec: &FileSpec) -> Result<Ledger> {
    let content: String = read_file(file_spec)?;
    ledger_parser::parse(&content).map_err(Into::into)
}

pub fn write_file(file_spec: &FileSpec, content: &str) -> Result<()> {
    let mut f = file_spec.writer()?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

pub fn write_ledger_file(file_spec: &FileSpec, ledger: &Ledger) -> Result<()> {
    let content: String = format!("{}", ledger);
    write_file(file_spec, &content)
}
