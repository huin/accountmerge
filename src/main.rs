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
struct Command {
    #[structopt(subcommand)]
    subcmd: SubCommand,
}

#[derive(Debug, StructOpt)]
enum SubCommand {
    #[structopt(name = "import")]
    Import(Import),
}

#[derive(Debug, StructOpt)]
struct Import {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
    #[structopt(short = "r", long = "rules")]
    rules: Option<PathBuf>,
}

fn main() -> Result<(), Error> {
    let cmd = Command::from_args();
    match cmd.subcmd {
        SubCommand::Import(import) => {
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
        }
    }
    Ok(())
}
