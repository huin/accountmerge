use std::collections::HashMap;

use crate::bank::InputTransaction;

struct Table {
    chains: HashMap<String, RuleChain>,
}

struct RuleChain {
    rules: Vec<Rule>,
}

impl RuleChain {}

struct Rule {
    predicate: Predicate,
    action: Action,
}

enum RuleResult {
    Return,

}

enum Action {
    JumpChain(String),
    Return,
    SetSrcAccount(String),
    SetDestAccount(String),
}

enum Predicate {
    True,
    SrcBank(StringMatch),
    SrcAcct(StringMatch),
}

enum StringMatch {
    Eq(String),
}
