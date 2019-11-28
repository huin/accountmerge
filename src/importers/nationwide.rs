use std::str::FromStr;

use failure::Error;

use crate::fingerprint::FingerprintBuilder;

pub const BANK_NAME: &str = "Nationwide";

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
    fn from_str(s: &str) -> Result<Self, Error> {
        use FpPrefix::*;

        match s {
            "account-name" => Ok(AccountName),
            "generated" => Ok(Generated),
            s if s.starts_with(FIXED_PREFIX) => Ok(Fixed(s[FIXED_PREFIX.len()..].to_string())),
            _ => bail!("invalid value for fingerprint prefix: {:?}", s),
        }
    }
}
