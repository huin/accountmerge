use anyhow::Result;

use clap::Args;

use crate::filespec::{self, FileSpec};
use crate::fingerprint;
use crate::internal::TransactionPostings;
use crate::tags;

#[derive(Debug, Args)]
pub struct Cmd {
    /// The Ledger journals to update.
    journals: Vec<FileSpec>,
}

impl Cmd {
    pub fn run(&self) -> Result<()> {
        for ledger_file in &self.journals {
            let ledger = filespec::read_ledger_file(ledger_file)?;
            let mut trns = TransactionPostings::from_ledger(ledger)?;
            update_transactions(&mut trns);
            let ledger = TransactionPostings::into_ledger(trns);
            filespec::write_ledger_file(ledger_file, &ledger)?;
        }

        Ok(())
    }
}

fn update_transactions(trns: &mut Vec<TransactionPostings>) {
    for trn in trns {
        for post in &mut trn.posts {
            if !post
                .comment
                .tags
                .iter()
                .map(String::as_str)
                .any(fingerprint::is_fingerprint)
            {
                // The post has no existing fingerprint tag. Add a
                // randomly generated one as requested.
                post.comment.tags.insert(format!(
                    "{}uuidb64-{}",
                    tags::FINGERPRINT_PREFIX,
                    uuid_b64::UuidB64::new().to_istring()
                ));
            }
        }
    }
}
