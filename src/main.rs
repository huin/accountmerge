use anyhow::Result;
use structopt::StructOpt;

#[cfg(test)]
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
mod tzabbr;

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
    ApplyRules(rules::cmd::Command),
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

fn main() -> Result<()> {
    let cmd = Command::from_args();
    use SubCommand::*;
    match cmd.subcmd {
        ApplyRules(cmd) => cmd.run(),
        GenerateFingerprints(cmd) => cmd.run(),
        Import(cmd) => cmd.run(),
        Merge(cmd) => cmd.run(),
    }
}
