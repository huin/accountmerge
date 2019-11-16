# Account Merge

Tools for managing Ledger journals.

## Matching algorithm

For each transaction in the source, scan over each of its postings in turn to
find an existing posting in the destination according to "Existing posting
lookup" as described below. Use this posting to determine a *default
destination transaction*:

*   The first destination posting that matches, use its transaction as the
    default destination transaction.
*   If no posting matches, create a new destination transaction as the default
    destination transaction.

Now process each source posting again to find its match in the destination,
again according to "Existing posting lookup".

*   When a match is found, update the matching posting in the destination by
    adding the tags (including fingerprint key+value). If the source posting
    does not have the "unknown-account" tag and the destination does, then
    additionally copy the account name from source to destination and remove
    the "unknown-account" tag from the destination.
*   If nothing matched, create a copy of the source posting within the *default
    destination transaction*.

This may create unbalanced transactions, which is left to be manually resolved.
So the user should run a check with the `ledger` command before continuing.

### Existing posting lookup

For each source posting being merged in, look for a possible existing posting
in the following order:

1.  Match based on fingerprint.

    Look for existing posting(s) that have the same fingerprint tag(s) from
    the source posting:

    *   If no fingerprints match any existing postings, continue to step 2.
    *   If only one posting is found, then use that as the destination posting.
    *   If multiple postings are found, this is an error.

2. Soft match based on the following non-fingerprint values:

    *   Same date on parent transaction.
    *   Same amount.
    *   If *both* source and destinations postings have a balance value, they
        must have the same balance.
    *   If *both* source and destination postings do *not* have the
        "unknown-account" tag, they must also match account names.

    This may match zero or more postings:

    *   If no postings match, then that is the end of the search and no
        existing postings are found to match.
    *   If only one posting is found, then use that as the destination posting.
    *   If multiple postings are found, then mark the existing matches with
        a tag `"candidate-$FINGERPRINT"` using a fingerprint from the source
        posting, and skip any further steps of merging this posting. It is an
        error if the source posting has no fingerprint.

        It is left for the user to select which of the existing postings it
        should be merged into by:

        1.  Removing the `candidate-` prefix from the tag on one of the
            existing postings.
        2.  Removing the `candidate-$FINGERPRINT` tags completely from other
            existing postings.
        3.  Re-running the merge tool.
