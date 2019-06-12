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

use std::path::PathBuf;

use failure::Error;
use ledger_parser::{Amount, Transaction};
use structopt::StructOpt;

mod bank;
mod builder;
mod rule;

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
    let input_transactions = bank::nationwide::transactions_from_path(&opt.input)?;
    let output_transactions_result: Result<Vec<_>, Error> = input_transactions
        .iter()
        .map(|in_trn| to_ledger(&rules, in_trn))
        .collect();
    let output_transactions = output_transactions_result?;
    for trn in &output_transactions {
        println!("{}", trn);
    }
    Ok(())
}

// temporary function to test output format. It's not (entirely) useful until we're labelling
// accounts properly.
fn to_ledger(rules: &rule::Table, in_trn: &bank::InputTransaction) -> Result<Transaction, Error> {
    let cmp = rules.derive_components(&in_trn)?;

    let source_account = cmp
        .source_account
        .unwrap_or_else(|| "assets:unknown".to_string());
    let peer_account = cmp.dest_account.unwrap_or_else(|| {
        if in_trn.paid.quantity.is_sign_positive() {
            "income:unknown".to_string()
        } else {
            "expenses:unknown".to_string()
        }
    });
    let peer_amount = Amount {
        quantity: -in_trn.paid.quantity,
        commodity: in_trn.paid.commodity.clone(),
    };

    let trn = builder::TransactionBuilder::new(
        in_trn.date,
        format!("{} - {}", in_trn.type_, in_trn.description),
    )
    .posting(
        source_account,
        in_trn.paid.clone(),
        Some(in_trn.balance.clone()),
    )
    .posting(peer_account, peer_amount, None)
    .build();
    Ok(trn)
}
