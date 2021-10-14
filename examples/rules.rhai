fn update_transaction(trn) {
    print(trn);
    trn.description += " plus suffix";
    trn.date.year = 2001;
    trn.date.month += 1;
    trn.date.day += 2;
    trn.comment.lines.push("Hello comment.");
    trn.comment.tags.push("tag");
    trn.comment.value_tags["key"] = "value";
    trn
}
