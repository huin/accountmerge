extern crate base64;
extern crate byteorder;
extern crate chrono;
extern crate chrono_tz;
extern crate csv;
extern crate encoding_rs;
extern crate encoding_rs_io;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate failure_derive;
extern crate itertools;
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
extern crate uuid_b64;

#[cfg(test)]
extern crate goldenfile;
#[cfg(test)]
extern crate test_case;
#[cfg(test)]
extern crate text_diff;
#[cfg(test)]
extern crate textwrap;

use failure::Error;
use structopt::StructOpt;

#[cfg(test)]
#[macro_use]
mod testutil;

mod accounts;
mod comment;
mod filespec;
mod fingerprint;
mod fpgen;
mod importers;
mod internal;
mod merge;
mod mutcell;
mod rules;
mod tags;

#[derive(Debug, StructOpt)]
/// Utilities for working with Ledger journals.
struct Command {
    #[structopt(subcommand)]
    subcmd: SubCommand,
}

#[derive(Debug, StructOpt)]
enum SubCommand {
    #[structopt(name = "apply-rules")]
    /// Applies a rules file to an input file and dumps the results to stdout,
    ApplyRules(rules::Command),
    #[structopt(name = "generate-fingerprints")]
    /// Generates random fingerprints to the postings in the input file and
    /// writes them back out.
    GenerateFingerprints(fpgen::Command),
    #[structopt(name = "import")]
    /// Reads financial transaction data from a given source, converts them to
    /// Ledger transactions, and dumps them to stdout.
    Import(importers::cmd::Command),
    #[structopt(name = "merge")]
    /// Merges multiple Ledger journals together.
    Merge(merge::cmd::Command),
}

fn main() -> Result<(), Error> {
    let cmd = Command::from_args();
    use SubCommand::*;
    match cmd.subcmd {
        ApplyRules(cmd) => cmd.run(),
        GenerateFingerprints(cmd) => cmd.run(),
        Import(cmd) => cmd.run(),
        Merge(cmd) => cmd.run(),
    }
}
