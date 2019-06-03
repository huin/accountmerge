use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use failure::Error;

use crate::bank::InputTransaction;

const START_CHAIN: &str = "start";

#[derive(Debug, Fail)]
pub enum RuleError {
    #[fail(display = "chain {} not found", chain)]
    ChainNotFound { chain: String },
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct DerivedComponents {
    pub dest_account: Option<String>,
    pub source_account: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Table {
    chains: HashMap<String, Chain>,
}

impl Table {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let reader = File::open(path)?;
        let table: Table = ron::de::from_reader(reader)?;
        table.validate()?;
        Ok(table)
    }

    pub fn derive_components(
        &self,
        trn: &InputTransaction,
    ) -> Result<DerivedComponents, RuleError> {
        let start = self.get_chain(START_CHAIN)?;
        let mut cmp = DerivedComponents::default();
        start.apply(self, trn, &mut cmp)?;
        Ok(cmp)
    }

    fn get_chain(&self, name: &str) -> Result<&Chain, RuleError> {
        self.chains
            .get(name)
            .ok_or_else(|| RuleError::ChainNotFound {
                chain: name.to_string(),
            })
    }

    fn validate(&self) -> Result<(), RuleError> {
        self.get_chain(START_CHAIN)?;
        for (_, chain) in &self.chains {
            chain.validate(self)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
struct Chain {
    rules: Vec<Rule>,
}

impl Chain {
    fn apply(
        &self,
        table: &Table,
        trn: &InputTransaction,
        cmp: &mut DerivedComponents,
    ) -> Result<(), RuleError> {
        for rule in &self.rules {
            match rule.apply(table, trn, cmp)? {
                RuleResult::Continue => {}
                RuleResult::Return => break,
            }
        }
        Ok(())
    }

    fn validate(&self, table: &Table) -> Result<(), RuleError> {
        for r in &self.rules {
            r.validate(table)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct Rule {
    predicate: Predicate,
    action: Action,
    result: RuleResult,
}

impl Rule {
    fn apply(
        &self,
        table: &Table,
        trn: &InputTransaction,
        cmp: &mut DerivedComponents,
    ) -> Result<RuleResult, RuleError> {
        if self.predicate.is_match(trn) {
            self.action.apply(table, trn, cmp)?;
            Ok(self.result)
        } else {
            Ok(RuleResult::Continue)
        }
    }

    fn validate(&self, table: &Table) -> Result<(), RuleError> {
        self.action.validate(table)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum RuleResult {
    Continue,
    Return,
}

#[derive(Debug, Deserialize)]
enum Action {
    Noop,
    JumpChain(String),
    SetDestAccount(String),
    SetSrcAccount(String),
}

impl Action {
    fn apply(
        &self,
        table: &Table,
        trn: &InputTransaction,
        cmp: &mut DerivedComponents,
    ) -> Result<(), RuleError> {
        use Action::*;

        match self {
            Noop => {}
            JumpChain(name) => {
                table.get_chain(name)?.apply(table, trn, cmp)?;
            }
            SetDestAccount(v) => {
                cmp.dest_account = Some(v.clone());
            }
            SetSrcAccount(v) => {
                cmp.source_account = Some(v.clone());
            }
        }

        Ok(())
    }

    fn validate(&self, table: &Table) -> Result<(), RuleError> {
        use Action::*;

        match self {
            JumpChain(name) => table.get_chain(name).map(|_| ()),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Deserialize)]
enum Predicate {
    True,
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    InputAccountName(StringMatch),
    InputBank(StringMatch),
    Not(Box<Predicate>),
}

impl Predicate {
    fn is_match(&self, trn: &InputTransaction) -> bool {
        use Predicate::*;
        match self {
            True => true,
            All(preds) => preds.iter().all(|p| p.is_match(trn)),
            Any(preds) => preds.iter().any(|p| p.is_match(trn)),
            InputAccountName(matcher) => matcher.matches_string(&trn.account_name),
            InputBank(matcher) => matcher.matches_string(&trn.bank),
            Not(pred) => !pred.is_match(trn),
        }
    }

    #[cfg(test)]
    fn not(self) -> Self {
        Predicate::Not(Box::new(self))
    }
}

#[derive(Debug, Deserialize)]
enum StringMatch {
    Eq(String),
}

impl StringMatch {
    fn matches_string(&self, s: &str) -> bool {
        use StringMatch::*;

        match self {
            Eq(want) => want == s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::NaiveDate;

    use crate::bank::Paid;
    use crate::money::{GbpValue, UnsignedGbpValue};

    struct TableBuilder {
        table: Table,
    }
    impl TableBuilder {
        fn new() -> Self {
            TableBuilder {
                table: Table::default(),
            }
        }

        fn chain(mut self, name: &str, chain: Chain) -> Self {
            self.table.chains.insert(name.to_string(), chain);
            self
        }

        fn build(self) -> Table {
            self.table
        }
    }

    struct ChainBuilder {
        chain: Chain,
    }
    impl ChainBuilder {
        fn new() -> Self {
            ChainBuilder {
                chain: Chain::default(),
            }
        }

        fn rule(mut self, action: Action, predicate: Predicate, result: RuleResult) -> Self {
            self.chain.rules.push(Rule {
                action,
                predicate,
                result,
            });
            self
        }

        fn build(self) -> Chain {
            self.chain
        }
    }

    /// Build an `InputTransaction` for testing.
    struct InputTransactionBuilder {
        trn: InputTransaction,
    }
    impl InputTransactionBuilder {
        fn new() -> Self {
            InputTransactionBuilder {
                trn: InputTransaction {
                    bank: "foo bank".to_string(),
                    account_name: "foo account".to_string(),
                    date: NaiveDate::from_ymd(2000, 1, 5),
                    type_: "Withdrawal".to_string(),
                    description: "".to_string(),
                    paid: Paid::In(UnsignedGbpValue::from_pence(100)),
                    balance: GbpValue::from_pence(200),
                },
            }
        }

        fn account_name(mut self, account_name: &str) -> Self {
            self.trn.account_name = account_name.to_string();
            self
        }

        fn bank(mut self, bank: &str) -> Self {
            self.trn.bank = bank.to_string();
            self
        }

        fn build(self) -> InputTransaction {
            self.trn
        }
    }

    struct DerivedComponentsBuilder {
        cmp: DerivedComponents,
    }
    impl DerivedComponentsBuilder {
        fn new() -> Self {
            DerivedComponentsBuilder {
                cmp: DerivedComponents::default(),
            }
        }

        fn dest_account(mut self, account: &str) -> Self {
            self.cmp.dest_account = Some(account.to_string());
            self
        }

        fn source_account(mut self, account: &str) -> Self {
            self.cmp.source_account = Some(account.to_string());
            self
        }

        fn build(self) -> DerivedComponents {
            self.cmp
        }
    }

    fn jump_chain(chain: &str) -> Action {
        Action::JumpChain(chain.to_string())
    }

    #[test]
    fn apply() {
        use RuleResult::*;
        struct Test {
            name: &'static str,
            table: Table,
            cases: Vec<Case>,
        };
        struct Case {
            trn: InputTransaction,
            want: DerivedComponents,
        }
        let tests = vec![
            Test {
                name: "empty chain",
                table: TableBuilder::new().chain("start", Chain::default()).build(),
                cases: vec![Case {
                    trn: InputTransactionBuilder::new().build(),
                    want: DerivedComponents::default(),
                }],
            },
            Test {
                name: "set source account",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(
                                Action::SetSrcAccount("foo".to_string()),
                                Predicate::True,
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![Case {
                    trn: InputTransactionBuilder::new().build(),
                    want: DerivedComponentsBuilder::new()
                        .source_account("foo")
                        .build(),
                }],
            },
            Test {
                name: "set dest account",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(
                                Action::SetDestAccount("bar".to_string()),
                                Predicate::True,
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![Case {
                    trn: InputTransactionBuilder::new().build(),
                    want: DerivedComponentsBuilder::new().dest_account("bar").build(),
                }],
            },
            Test {
                name: "set account in jumped chain",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(jump_chain("some-chain"), Predicate::True, Continue)
                            .build(),
                    )
                    .chain(
                        "some-chain",
                        ChainBuilder::new()
                            .rule(
                                Action::SetSrcAccount("foo".to_string()),
                                Predicate::True,
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![Case {
                    trn: InputTransactionBuilder::new().build(),
                    want: DerivedComponentsBuilder::new()
                        .source_account("foo")
                        .build(),
                }],
            },
            Test {
                name: "return before set account",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(Action::Noop, Predicate::True, Return)
                            .rule(
                                Action::SetSrcAccount("foo".to_string()),
                                Predicate::True,
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![Case {
                    trn: InputTransactionBuilder::new().build(),
                    want: DerivedComponentsBuilder::new().build(),
                }],
            },
            Test {
                name: "set account based on input account",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(
                                Action::SetSrcAccount("assets::foo".to_string()),
                                Predicate::InputAccountName(StringMatch::Eq("foo".to_string())),
                                Continue,
                            )
                            .rule(
                                Action::SetSrcAccount("assets::bar".to_string()),
                                Predicate::InputAccountName(StringMatch::Eq("bar".to_string())),
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![
                    Case {
                        trn: InputTransactionBuilder::new().account_name("foo").build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::foo")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new().account_name("bar").build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::bar")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new().account_name("quux").build(),
                        want: DerivedComponentsBuilder::new().build(),
                    },
                ],
            },
            Test {
                name: "set account based on input bank",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(
                                Action::SetSrcAccount("assets::foo".to_string()),
                                Predicate::InputBank(StringMatch::Eq("foo".to_string())),
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![
                    Case {
                        trn: InputTransactionBuilder::new().bank("foo").build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::foo")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new().bank("bar").build(),
                        want: DerivedComponentsBuilder::new().build(),
                    },
                ],
            },
            Test {
                name: "set account based on various boolean conditions",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(
                                Action::SetSrcAccount("assets::acct1-bank1".to_string()),
                                Predicate::All(vec![
                                    Predicate::InputAccountName(StringMatch::Eq(
                                        "acct1".to_string(),
                                    )),
                                    Predicate::InputBank(StringMatch::Eq("bank1".to_string())),
                                ]),
                                Continue,
                            )
                            .rule(
                                Action::SetSrcAccount("assets::acct1-bank2".to_string()),
                                Predicate::All(vec![
                                    Predicate::InputAccountName(StringMatch::Eq(
                                        "acct1".to_string(),
                                    )),
                                    Predicate::InputBank(StringMatch::Eq("bank2".to_string())),
                                ]),
                                Continue,
                            )
                            .rule(
                                Action::SetSrcAccount("assets::acct2-bank1".to_string()),
                                Predicate::All(vec![
                                    Predicate::InputAccountName(StringMatch::Eq(
                                        "acct2".to_string(),
                                    )),
                                    Predicate::InputBank(StringMatch::Eq("bank1".to_string())),
                                ]),
                                Continue,
                            )
                            .rule(
                                Action::SetSrcAccount("assets::acct2-bank2".to_string()),
                                Predicate::All(vec![
                                    Predicate::InputAccountName(StringMatch::Eq(
                                        "acct2".to_string(),
                                    )),
                                    Predicate::InputBank(StringMatch::Eq("bank2".to_string())),
                                ]),
                                Continue,
                            )
                            .rule(
                                Action::SetSrcAccount("assets::acct3-or-4".to_string()),
                                Predicate::Any(vec![
                                    Predicate::InputAccountName(StringMatch::Eq(
                                        "acct3".to_string(),
                                    )),
                                    Predicate::InputAccountName(StringMatch::Eq(
                                        "acct4".to_string(),
                                    )),
                                ]),
                                Continue,
                            )
                            .rule(
                                Action::SetSrcAccount("assets::acct-other-bank1".to_string()),
                                Predicate::All(vec![
                                    Predicate::InputAccountName(StringMatch::Eq(
                                        "acct1".to_string(),
                                    ))
                                    .not(),
                                    Predicate::InputAccountName(StringMatch::Eq(
                                        "acct2".to_string(),
                                    ))
                                    .not(),
                                    Predicate::InputBank(StringMatch::Eq("bank1".to_string())),
                                ]),
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![
                    Case {
                        trn: InputTransactionBuilder::new()
                            .account_name("acct1")
                            .bank("unmatched")
                            .build(),
                        // Fallthrough without matching any rules.
                        want: DerivedComponentsBuilder::new().build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new()
                            .account_name("acct1")
                            .bank("bank1")
                            .build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::acct1-bank1")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new()
                            .account_name("acct1")
                            .bank("bank2")
                            .build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::acct1-bank2")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new()
                            .account_name("acct2")
                            .bank("bank1")
                            .build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::acct2-bank1")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new()
                            .account_name("acct2")
                            .bank("bank2")
                            .build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::acct2-bank2")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new().account_name("acct3").build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::acct3-or-4")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new().account_name("acct4").build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::acct3-or-4")
                            .build(),
                    },
                    Case {
                        trn: InputTransactionBuilder::new()
                            .account_name("acct3")
                            .bank("bank1")
                            .build(),
                        want: DerivedComponentsBuilder::new()
                            .source_account("assets::acct-other-bank1")
                            .build(),
                    },
                ],
            },
        ];

        for test in &tests {
            for (i, case) in test.cases.iter().enumerate() {
                let cmp = test.table.derive_components(&case.trn).unwrap();
                assert_eq!(case.want, cmp, "for test {}#{}", test.name, i);
            }
        }
    }

    #[test]
    fn validate_valid_tables() {
        use RuleResult::*;
        struct Test(&'static str, Table);
        let tests = vec![
            Test(
                "empty start chain",
                TableBuilder::new().chain("start", Chain::default()).build(),
            ),
            Test(
                "jump to other chain",
                TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(jump_chain("foo"), Predicate::True, Continue)
                            .build(),
                    )
                    .chain("foo", Chain::default())
                    .build(),
            ),
        ];

        for t in &tests {
            t.1.validate().expect(&format!("{} => should succeed", t.0));
        }
    }

    #[test]
    fn validate_invalid_tables() {
        use RuleResult::*;
        struct Test(&'static str, Table);
        let tests = vec![
            Test(
                "no start chain",
                TableBuilder::new().chain("foo", Chain::default()).build(),
            ),
            Test(
                "jump to non existing chain",
                TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(jump_chain("foo"), Predicate::True, Continue)
                            .build(),
                    )
                    .chain(
                        "foo",
                        ChainBuilder::new()
                            .rule(jump_chain("not-exist"), Predicate::True, Continue)
                            .build(),
                    )
                    .build(),
            ),
        ];

        for t in &tests {
            t.1.validate()
                .expect_err(&format!("{} => should fail", t.0));
        }
    }
}
