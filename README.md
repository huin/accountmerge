
## Merging Ideas:

* Postings merged in that have an ID (a generated hash) are first checked to
  see if that ID already exists, if it does not, then we fall back to some search algorithm and fallback to ultimately copying the whole transaction over.
* Ignore transaction description in comparing. Instead look at the transaction
  date (as before), and also see if the postings in the transaction match other postings in the current posting.
