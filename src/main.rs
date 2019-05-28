extern crate chrono;
extern crate csv;
extern crate encoding_rs;
extern crate encoding_rs_io;
extern crate failure;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate lazy_static;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate structopt;

use std::path::PathBuf;

use structopt::StructOpt;

mod bank;
mod money;
mod output;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
}

fn main() -> Result<(), Box<std::error::Error>> {
    let opt = Opt::from_args();
    let input_transactions = bank::nationwide::transactions_from_path(&opt.input)?;
    for record in &input_transactions {
        println!("{:?}", record);
    }
    let output_transactions: Vec<_> = input_transactions.into_iter().map(to_output).collect();
    for trn in &output_transactions {
        println!("{}", trn);
    }
    Ok(())
}

// temporary function to test output format. It's not (entirely) useful until we're labelling
// accounts properly.
fn to_output(in_trn: bank::InputTransaction) -> output::Transaction {
    use bank::Paid;
    use output::*;
    Transaction {
        date: in_trn.date,
        description: format!("{} - {}", in_trn.type_, in_trn.description),
        postings: vec![
            Posting {
                account: format!("assets::account::{}", in_trn.src_acct),
                amount: match in_trn.paid {
                    Paid::In(v) => v,
                    Paid::Out(v) => -v,
                },
            },
            Posting {
                account: if let Paid::In(_) = in_trn.paid {
                    "income::unknown".to_string()
                } else {
                    "expenses::unknown".to_string()
                },
                amount: match in_trn.paid {
                    Paid::In(v) => -v,
                    Paid::Out(v) => v,
                },
            },
        ],
    }
}
