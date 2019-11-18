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
mod merge;
mod rule;
mod tags;

use filespec::FileSpec;

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
    ApplyRules(ApplyRules),
    #[structopt(name = "generate-fingerprints")]
    /// Generates random fingerprints to the postings in the input file and
    /// writes them back out.
    GenerateFingerprints(fpgen::Command),
    #[structopt(name = "import")]
    /// Reads financial transaction data from a given source, converts them to
    /// Ledger transactions, and dumps them to stdout.
    Import(importers::Command),
    #[structopt(name = "merge")]
    /// Merges multiple Ledger journals together.
    Merge(merge::Command),
}

fn main() -> Result<(), Error> {
    let cmd = Command::from_args();
    use SubCommand::*;
    match cmd.subcmd {
        ApplyRules(apply_rules) => do_apply_rules(&apply_rules),
        GenerateFingerprints(gen_fp) => gen_fp.run(),
        Import(import) => import.run(),
        Merge(merge) => merge.run(),
    }
}

#[derive(Debug, StructOpt)]
struct ApplyRules {
    #[structopt(short = "r", long = "rules")]
    /// The file to read the rules from.
    rules: FileSpec,
    /// The Ledger journal to read.
    input_journal: FileSpec,
    /// The ledger file to write to (overwrites any existing file). "-" writes
    /// to stdout.
    #[structopt(short = "o", long = "output", default_value = "-")]
    output: FileSpec,
}

fn do_apply_rules(apply_rules: &ApplyRules) -> Result<(), Error> {
    let mut ledger = filespec::read_ledger_file(&apply_rules.input_journal)?;
    let rules = rule::Table::from_filespec(&apply_rules.rules)?;
    for trn in &mut ledger.transactions {
        rules.update_transaction(trn)?;
    }
    filespec::write_ledger_file(&apply_rules.output, &ledger)
}
