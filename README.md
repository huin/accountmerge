# Account Merge

Tools for managing Ledger journals.

## Disclaimer

This code hasn't been substantially battle-tested. It may well corrupt
accounting files. As such, it's best to:

- keep backups of your account journals,
- preview change made to your journal made by this utility before committing
  to them,
- make use of balance assertions where possible to help catch mistakes (such
  as duplicated transactions/postings).

The first two recommendations naturally suggest using a version control tool
(such as Git, Mercurial).

## Required Features

- `merge`:
  - Configurable window to match transactions. It's common for a
    transfer between accounts to be delayed. This shouldn't result in
    duped transactions.
  - Be smarter about merging transactions in. It's commonly incorrect to merge
    a transaction into an existing transaction if one posting gets deduped
    against an existing posting, and another posting gets created/added.
  - Retain ordering from existing transaction sources. Might need to import
    with a tag that specifies the order of a transaction within the given date.
    Re-order in the destination to respect the source.

## Fingerprints

Posting fingerprints are created by the `import` subcommands and by the
`generate-fingerprints` subcommands. These are intended for use by the `merge`
subcommand. The matching algorithm (described below) goes into detail, but the
core detail is that for a given fingerprint prefix, the fingerprint value
attempts to uniquely identify a posting between `merge` runs, so that a posting
that is re-imported is recognized in the existing journal.

### Overall structure

A fingerprint namespace identifies:

- The type and version of fingerprint generation algorithm.
- The data source of the imported transaction data (typically a short unique
  name for the bank account).

A fingerprint is a posting tag string resembling `fp-namespace-value`, consisting of 3 parts
separated by hyphens (`-`), e.g:

1. A fixed `fp-` prefix, which identifies the Ledger tag as a fingerprint.
2. An identifying "namespace" (see below for more details).
3. The generated fingerprint value, typically encoded in base64, often a hash
   of data from the original record data that generated the posting, or a random
   UUID.

The contents of the namespace and value parts are opaque to the `merge` subcommand.

### Import namespace structure

In the case where the "namespace" is generated by an import algorithm (see
`fingerprint::FingerprintBuilder::build_tag()`), this prefix consists of 3 parts (e.g
`algorithm.version.uservalue`), separated by periods (`.`), e.g for `nwcsv6.1.checking`
would be made of the parts:

1. The name and version of the fingerprint algorithm, e.g `nwcsv6` for "Nationwide CSV 6 column".
2. The version of the algorithm, e.g `1`.
3. A user provided value, which typically uniquely names one of their bank
   accounts, e.g `checking`.

## Matching algorithm

For each transaction in the source, scan over each of its postings in turn to
find an existing posting in the destination according to "Existing posting
lookup" as described below. Use this posting to determine a _default
destination transaction_:

- The first destination posting that matches, use its transaction as the
  default destination transaction.
- If no posting matches, create a new destination transaction as the default
  destination transaction.

Now process each source posting again to find its match in the destination,
again according to "Existing posting lookup".

- When a match is found, update the matching posting in the destination by
  adding the tags (including fingerprint key+value). If the source posting
  does not have the "unknown-account" tag and the destination does, then
  additionally copy the account name from source to destination and remove
  the "unknown-account" tag from the destination.
- If nothing matched, create a copy of the source posting within the _default
  destination transaction_.

This may create unbalanced transactions, which is left to be manually resolved.
So the user should run a check with the `ledger` command before continuing.

### Existing posting lookup

For each source posting being merged in, look for a possible existing posting
in the following order:

1. Match based on fingerprint.

   Look for existing posting(s) that have the same fingerprint tag(s) from the
   source posting:

   - If no fingerprints match any existing postings, continue to step 2.
   - If only one posting is found, then use that as the destination posting.
   - If multiple postings are found, this is an error.

2. Soft match based on the following non-fingerprint values:

   - Same date on parent transaction.
   - Same amount.
   - If _both_ source and destinations postings have a balance value, they
     must have the same balance.
   - If _both_ source and destination postings do _not_ have the
     "unknown-account" tag, they must also match account names.

   This may match zero or more postings:

   - If no postings match, then that is the end of the search and no existing
     postings are found to match.
   - If only one posting is found, then use that as the destination posting.
   - If multiple postings are found, then mark the source posting with tags in
     the form `"candidate-$FINGERPRINT"` using a fingerprint from the
     potential destination postings, and skip any further steps of merging
     this posting. The source posting's parent transaction will then go into
     the separate "unmerged" output.

     It is left for the user to select which of the existing postings it
     should be merged into by:

     1. Editing the unmerged output file:
        - Removing the `candidate-` prefix from one of the tags on one of
          the unmerged postings to identify which destination tag it should
          merge into.
        - Removing the other `candidate-$FINGERPRINT` tags completely.
     2. Re-running the merge tool to include the edited unmerged
        transactions file.
