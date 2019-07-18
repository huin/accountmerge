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

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
    #[structopt(short = "r", long = "rules")]
    rules: PathBuf,
}

fn main() -> Result<(), Error> {
    let opt = Opt::from_args();
    let rules = rule::Table::from_path(&opt.rules)?;
    let mut transactions = bank::nationwide::transactions_from_path(&opt.input)?;
    for trn in &mut transactions {
        println!("=========");
        println!("BEFORE: {}", trn);
        rules.update_transaction(trn)?;
        println!("AFTER: {}", trn);
    }
    Ok(())
}
