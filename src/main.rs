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

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(parse(from_os_str))]
    input: PathBuf,
}

fn main() -> Result<(), Box<std::error::Error>> {
    let opt = Opt::from_args();
    let transactions = bank::nationwide::transactions_from_path(&opt.input)?;
    for record in &transactions {
        println!("{:?}", record);
    }
    Ok(())
}
