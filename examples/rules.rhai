fn update_transaction(trn) {
    print(trn);
    trn.description += " plus suffix";
    trn.date.year += 1;
    trn.date.month += 1;
    trn.date.day += 2;
    trn.comment.lines.push("Hello transaction comment.");
    trn.postings[0].comment.lines.push("Hello postings comment.");
    trn.postings[0].amount.quantity += 1;
    trn.comment.tags.push("transaction-tag");
    trn.comment.value_tags["key"] = "value";
    trn
}
