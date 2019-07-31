extern crate chrono;
extern crate csv;
extern crate encoding_rs;
extern crate encoding_rs_io;
extern crate failure;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate lazy_static;
extern crate ledger_parser;
extern crate regex;
extern crate ron;
extern crate rust_decimal;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate structopt;

#[cfg(test)]
extern crate text_diff;
#[cfg(test)]
extern crate textwrap;

use std::io::Read;
use std::path::PathBuf;

use failure::Error;
use structopt::StructOpt;

#[cfg(test)]
#[macro_use]
mod testutil;

mod bank;
mod merge;
mod rule;
mod tags;

#[derive(Debug, Fail)]
enum MergeError {
    #[fail(display = "parse error: {}", reason)]
    ParseError { reason: String },
    #[fail(display = "unmerged transactions:\n{}", unmerged)]
    UnmergedError { unmerged: ledger_parser::Ledger },
}

#[derive(Debug, StructOpt)]
struct Command {
    #[structopt(subcommand)]
    subcmd: SubCommand,
}

#[derive(Debug, StructOpt)]
enum SubCommand {
    #[structopt(name = "import")]
    Import(Import),
    #[structopt(name = "merge")]
    Merge(Merge),
}

fn main() -> Result<(), Error> {
    let cmd = Command::from_args();
    use SubCommand::*;
    match cmd.subcmd {
        Import(import) => do_import(&import),
        Merge(merge) => do_merge(&merge),
    }
}

#[derive(Debug, StructOpt)]
struct Import {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
    #[structopt(short = "r", long = "rules")]
    rules: Option<PathBuf>,
}

fn do_import(import: &Import) -> Result<(), Error> {
    let mut transactions = bank::nationwide::transactions_from_path(&import.input)?;
    if let Some(rules_path) = &import.rules {
        let rules = rule::Table::from_path(rules_path)?;
        for trn in &mut transactions {
            rules.update_transaction(trn)?;
        }
    }
    let ledger = ledger_parser::Ledger {
        transactions: transactions,
        commodity_prices: Default::default(),
    };
    println!("{}", ledger);
    Ok(())
}

#[derive(Debug, StructOpt)]
struct Merge {
    #[structopt(parse(from_os_str))]
    inputs: Vec<PathBuf>,
}

fn do_merge(merge: &Merge) -> Result<(), Error> {
    let mut merger = merge::Merger::new();

    for path in &merge.inputs {
        let ledger = read_from_file(&path)?;
        merger.merge(ledger.transactions);
    }

    let result = merger.build();

    if !result.unmerged.is_empty() {
        return Err(MergeError::UnmergedError {
            unmerged: ledger_parser::Ledger {
                transactions: result.unmerged,
                commodity_prices: Default::default(),
            },
        }
        .into());
    }

    let ledger = ledger_parser::Ledger {
        transactions: result.merged,
        commodity_prices: Default::default(),
    };
    println!("{}", ledger);

    Ok(())
}

fn read_from_file(path: &std::path::Path) -> Result<ledger_parser::Ledger, Error> {
    let mut content = String::new();
    let mut f = std::fs::File::open(path)?;
    f.read_to_string(&mut content)?;

    ledger_parser::parse(&content).map_err(|e| MergeError::ParseError { reason: e }.into())
}
