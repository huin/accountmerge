use std::str::FromStr;

use anyhow::{Error, Result};
use structopt::StructOpt;

use crate::fingerprint::FingerprintBuilder;

pub const BANK_NAME: &str = "Nationwide";

/// Common options for Nationwide importers.
#[derive(Debug, StructOpt)]
pub struct CommonOpts {
    /// The prefix of the fingerprints to generate (without "fp-" that will be
    /// prefixed to this value).
    ///
    /// "account-name" uses the account name from the CSV file.
    ///
    /// "fixed:<prefix>" uses the given fixed prefix.
    ///
    /// "generated" generates a hashed value based on the account name in the
    /// CSV file.
    #[structopt(long = "fingerprint-prefix", default_value = "generated")]
    pub fp_prefix: FpPrefix,
}

#[derive(Debug)]
pub enum FpPrefix {
    AccountName,
    Fixed(String),
    Generated,
}

impl FpPrefix {
    pub fn to_prefix(&self, account_name: &str) -> String {
        use FpPrefix::*;

        match self {
            AccountName => account_name.to_string(),
            Fixed(s) => s.clone(),
            Generated => {
                let mut s = FingerprintBuilder::new()
                    .with(BANK_NAME)
                    .with(account_name)
                    .build();
                s.truncate(8);
                s
            }
        }
    }
}

const FIXED_PREFIX: &str = "fixed:";

impl FromStr for FpPrefix {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        use FpPrefix::*;

        match s {
            "account-name" => Ok(AccountName),
            "generated" => Ok(Generated),
            s if s.starts_with(FIXED_PREFIX) => Ok(Fixed(s[FIXED_PREFIX.len()..].to_string())),
            _ => bail!("invalid value for fingerprint prefix: {:?}", s),
        }
    }
}
