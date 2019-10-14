extern crate base64;
extern crate byteorder;
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
extern crate sha1;
extern crate structopt;
extern crate typed_generational_arena;

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

mod accounts;
mod bank;
mod comment;
mod fingerprint;
mod merge;
mod rule;
mod tags;

#[derive(Debug, Fail)]
enum CommandError {
    #[fail(display = "parse error: {}", reason)]
    ParseError { reason: String },
}

#[derive(Debug, StructOpt)]
struct Command {
    #[structopt(subcommand)]
    subcmd: SubCommand,
}

#[derive(Debug, StructOpt)]
enum SubCommand {
    #[structopt(name = "apply-rules")]
    ApplyRules(ApplyRules),
    #[structopt(name = "import")]
    Import(Import),
    #[structopt(name = "merge")]
    Merge(Merge),
}

fn main() -> Result<(), Error> {
    let cmd = Command::from_args();
    use SubCommand::*;
    match cmd.subcmd {
        ApplyRules(apply_rules) => do_apply_rules(&apply_rules),
        Import(import) => do_import(&import),
        Merge(merge) => do_merge(&merge),
    }
}

#[derive(Debug, StructOpt)]
struct ApplyRules {
    #[structopt(short = "r", long = "rules")]
    rules: PathBuf,
    #[structopt(parse(from_os_str))]
    input_journal: PathBuf,
}

fn do_apply_rules(apply_rules: &ApplyRules) -> Result<(), Error> {
    let mut ledger = read_from_file(&apply_rules.input_journal)?;
    let rules = rule::Table::from_path(&apply_rules.rules)?;
    for trn in &mut ledger.transactions {
        rules.update_transaction(trn)?;
    }
    println!("{}", ledger);
    Ok(())
}

#[derive(Debug, StructOpt)]
struct Import {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
}

fn do_import(import: &Import) -> Result<(), Error> {
    let transactions = bank::nationwide_csv::transactions_from_path(&import.input)?;
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
        merger.merge(ledger.transactions)?;
    }

    let transactions = merger.build();

    let ledger = ledger_parser::Ledger {
        transactions,
        commodity_prices: Default::default(),
    };
    println!("{}", ledger);

    Ok(())
}

fn read_from_file(path: &std::path::Path) -> Result<ledger_parser::Ledger, Error> {
    let mut content = String::new();
    let mut f = std::fs::File::open(path)?;
    f.read_to_string(&mut content)?;

    ledger_parser::parse(&content).map_err(|e| CommandError::ParseError { reason: e }.into())
}
