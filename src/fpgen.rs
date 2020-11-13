use anyhow::Result;

use structopt::StructOpt;

use crate::filespec::{self, FileSpec};
use crate::fingerprint;
use crate::internal::TransactionPostings;
use crate::tags;

#[derive(Debug, StructOpt)]
pub struct Command {
    /// The Ledger journals to update.
    journals: Vec<FileSpec>,
}

impl Command {
    pub fn run(&self) -> Result<()> {
        for ledger_file in &self.journals {
            let mut ledger = filespec::read_ledger_file(ledger_file)?;
            let mut trns = TransactionPostings::take_from_ledger(&mut ledger);
            update_transactions(&mut trns);
            TransactionPostings::put_into_ledger(&mut ledger, trns);
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
