use std::str::FromStr;

use anyhow::{Error, Result};
use structopt::StructOpt;

use crate::fingerprint::Accumulator;

pub const BANK_NAME: &str = "Nationwide";

/// Common options for Nationwide importers.
#[derive(Debug, StructOpt)]
pub struct CommonOpts {
    /// The user provided component of the fingerprint namespace. This
    /// typically uniquely identifies one of the user's accounts.
    ///
    /// "account-name" uses the account name from the CSV file.
    ///
    /// "fixed:<prefix>" uses the given fixed prefix.
    ///
    /// "generated" generates a hashed value based on the account name in the
    /// CSV file.
    #[structopt(long = "fp-namespace", default_value = "generated")]
    pub fp_ns: FpNamespace,
}

#[derive(Debug)]
pub enum FpNamespace {
    AccountName,
    Fixed(String),
    Generated,
}

impl FpNamespace {
    pub fn make_namespace(&self, account_name: &str) -> String {
        use FpNamespace::*;

        match self {
            AccountName => account_name.to_string(),
            Fixed(s) => s.clone(),
            Generated => {
                let mut s = Accumulator::new()
                    .with(BANK_NAME)
                    .with(account_name)
                    .into_base64();
                s.truncate(8);
                s
            }
        }
    }
}

const FIXED_PREFIX: &str = "fixed:";

impl FromStr for FpNamespace {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        use FpNamespace::*;

        match s {
            "account-name" => Ok(AccountName),
            "generated" => Ok(Generated),
            s if s.starts_with(FIXED_PREFIX) => Ok(Fixed(s[FIXED_PREFIX.len()..].to_string())),
            _ => bail!("invalid value for fingerprint namespace: {:?}", s),
        }
    }
}
