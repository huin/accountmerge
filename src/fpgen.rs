use failure::Error;
use ledger_parser::Transaction;
use structopt::StructOpt;

use crate::comment;
use crate::filespec::{self, FileSpec};
use crate::tags::FINGERPRINT_TAG_PREFIX;

#[derive(Debug, StructOpt)]
pub struct Command {
    /// The Ledger journals to update.
    journals: Vec<FileSpec>,
}

impl Command {
    pub fn run(&self) -> Result<(), Error> {
        for ledger_file in &self.journals {
            let mut ledger = filespec::read_ledger_file(ledger_file)?;
            update_transactions(&mut ledger.transactions);
            filespec::write_ledger_file(ledger_file, &ledger)?;
        }

        Ok(())
    }
}

fn update_transactions(trns: &mut Vec<Transaction>) {
    for trn in trns {
        for post in &mut trn.postings {
            let mut c =
                comment::Comment::from_opt_comment(post.comment.as_ref().map(String::as_str));
            if !c
                .tags
                .iter()
                .any(|tag| tag.starts_with(FINGERPRINT_TAG_PREFIX))
            {
                // The post has no existing fingerprint tag. Add a
                // randomly generated one as requested.
                c.tags.insert(format!(
                    "{}uuidb64-{}",
                    FINGERPRINT_TAG_PREFIX,
                    uuid_b64::UuidB64::new().to_istring()
                ));

                post.comment = c.into_opt_comment();
            }
        }
    }
}
