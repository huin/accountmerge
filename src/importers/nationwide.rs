use std::collections::HashMap;
use std::str::FromStr;

use anyhow::{anyhow, bail, Error, Result};
use structopt::StructOpt;

use crate::filespec::FileSpec;
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
    ///
    /// "lookup:<path>" reads the RON file at the given file (containing a
    /// `HashMap<String,String>`), and uses it to map from the account name
    /// in the CSV file to the fingerprint namespace.
    #[structopt(long = "fp-namespace", default_value = "generated")]
    pub fp_ns: FpNamespace,
}

#[derive(Debug)]
pub enum FpNamespace {
    AccountName,
    Fixed(String),
    Generated,
    Lookup(HashMap<String, String>),
}

impl FpNamespace {
    pub fn make_namespace(&self, account_name: &str) -> Result<String> {
        use FpNamespace::*;

        match self {
            AccountName => Ok(account_name.to_string()),
            Fixed(s) => Ok(s.clone()),
            Generated => {
                let mut s = Accumulator::new()
                    .with(BANK_NAME)
                    .with(account_name)
                    .into_base64();
                s.truncate(8);
                Ok(s)
            }
            Lookup(t) => t
                .get(account_name)
                .cloned()
                .ok_or_else(|| anyhow!("no account namespace found for {:?}", account_name)),
        }
    }
}

const FIXED_PREFIX: &str = "fixed:";
const LOOKUP_PREFIX: &str = "lookup:";

impl FromStr for FpNamespace {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        use FpNamespace::*;

        match s {
            "account-name" => Ok(AccountName),
            "generated" => Ok(Generated),
            s if s.starts_with(FIXED_PREFIX) => Ok(Fixed(s[FIXED_PREFIX.len()..].to_string())),
            s if s.starts_with(LOOKUP_PREFIX) => {
                let path = FileSpec::from_str(&s[LOOKUP_PREFIX.len()..])?;
                let reader = path.reader()?;
                let namespaces: HashMap<String, String> = ron::de::from_reader(reader)?;
                Ok(Lookup(namespaces))
            }
            _ => bail!("invalid value for fingerprint namespace: {:?}", s),
        }
    }
}
