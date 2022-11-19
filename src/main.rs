use anyhow::Result;
use clap::{Parser, Subcommand};

#[cfg(test)]
mod testutil;

mod accounts;
mod comment;
mod filespec;
mod fingerprint;
mod fpgen;
mod importers;
mod internal;
mod ledgerutil;
mod merge;
mod mutcell;
mod rules;
mod tags;
mod tzabbr;

#[derive(Debug, Parser)]
/// Utilities for working with Ledger journals.
struct Command {
    #[command(subcommand)]
    subcmd: SubCommand,
}

#[derive(Debug, Subcommand)]
enum SubCommand {
    #[command(name = "apply-rules")]
    /// Applies a rules file to an input file and dumps the results to stdout,
    ApplyRules(rules::cmd::Command),
    #[command(name = "generate-fingerprints")]
    /// Generates random fingerprints to the postings in the input file and
    /// writes them back out.
    GenerateFingerprints(fpgen::Cmd),
    #[command(name = "import")]
    /// Reads financial transaction data from a given source, converts them to
    /// Ledger transactions, and dumps them to stdout.
    Import(importers::cmd::Command),
    #[command(name = "merge")]
    /// Merges multiple Ledger journals together.
    Merge(merge::cmd::Command),
}

fn main() -> Result<()> {
    let cmd = Command::parse();
    use SubCommand::*;
    match cmd.subcmd {
        ApplyRules(cmd) => cmd.run(),
        GenerateFingerprints(cmd) => cmd.run(),
        Import(cmd) => cmd.run(),
        Merge(cmd) => cmd.run(),
    }
}
