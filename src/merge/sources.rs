use std::collections::HashMap;

use failure::Error;

use crate::filespec::{self, FileSpec};
use crate::internal::TransactionPostings;
use crate::tags::TRANSACTION_SOURCE_KEY;

/// Reads a Ledger file, and yields sets of `TransactionPostings` according to
/// how the transactions declare  where they came from based on their source
/// tags.
pub fn read_ledger_file(
    ledger_file: &FileSpec,
) -> Result<impl Iterator<Item = Vec<TransactionPostings>>, Error> {
    let mut ledger = filespec::read_ledger_file(ledger_file)?;
    let trns = TransactionPostings::take_from_ledger(&mut ledger);
    let default_source = format!("{}", ledger_file);

    let mut trns_by_source: HashMap<String, Vec<TransactionPostings>> = HashMap::new();
    for mut trn_posts in trns {
        // Ensure that incoming transactions are annotated with their source if
        // not already.
        let source = trn_posts
            .trn
            .comment
            .value_tags
            .entry(TRANSACTION_SOURCE_KEY.to_string())
            .or_insert_with(|| default_source.clone())
            .clone();
        // Group the transaction by its source.
        trns_by_source.entry(source).or_default().push(trn_posts);
    }

    let mut source_trn_posts: Vec<(String, Vec<TransactionPostings>)> =
        trns_by_source.into_iter().collect();
    // Sort by source.
    source_trn_posts.sort_by(|lhs, rhs| lhs.0.cmp(&rhs.0));

    Ok(source_trn_posts
        .into_iter()
        .map(|(_source, trn_posts)| trn_posts))
}

/// Remove all source tags from the transactions.
pub fn strip_sources(trns: &mut [TransactionPostings]) {
    for trn_posts in trns {
        trn_posts
            .trn
            .comment
            .value_tags
            .remove(TRANSACTION_SOURCE_KEY);
    }
}
