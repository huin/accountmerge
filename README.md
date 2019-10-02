## Fingerpring ideas:

*   For basic CSV sources that only include the date, include a counter of the
    transaction/posting within the day. Include this counter as part of the
    hash material.

## Matching algorithm

This algorithm is described as it is intended to be implemented, but is not
yet implemented.

For each transaction in the source, scan over each of its postings in turn to
find an existing posting in the destination according to "Existing posting
lookup" as described below. Use this posting to determine a *default
destination transaction*:

*   The first destination posting that matches, use its transaction as the
    default destination transaction.
*   If no posting matches, create a new destination transaction as the default
    destination transaction.

Now process each source transaction again to find its match in the destination,
again according to "Existing posting lookup".

*   When a match is found, update the matching posting in the destination by
    adding the tags (including fingerprint key+value). If the source posting
    has the "canonical" tag, then additionally set the account name.
*   If nothing matched, create a copy of the source posting within the *default
    destination transaction*, and add the tag "unmatched" to inform the user
    that manual intervention might be required on that posting.

This may create unbalanced transactions, which is left to be manually resolved.
So the user should run a check with the `ledger` command before continuing.

### Existing posting lookup

For each source posting being merged in, look for a possible existing posting
in the following order:

1.  Match based on fingerprint.

    *   Only one fingerprint is allowed for the posting being merged in.
    *   Look for an existing destination posting that has the same fingerprint
        key and value.

2. Match based on the following non-fingerprint values:

    *   Same date on parent transaction.
    *   Same amount.
