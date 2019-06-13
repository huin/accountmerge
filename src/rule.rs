use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use failure::Error;
use ledger_parser::{Posting, Transaction};

const START_CHAIN: &str = "start";

#[derive(Debug, Fail)]
pub enum RuleError {
    #[fail(display = "chain {} not found", chain)]
    ChainNotFound { chain: String },
}

pub struct PostingContext<'a> {
    trn: &'a mut Transaction,
    posting_idx: usize,
}

impl PostingContext<'_> {
    fn posting(&self) -> &Posting {
        &self.trn.postings[self.posting_idx]
    }

    fn mut_posting(&mut self) -> &mut Posting {
        &mut self.trn.postings[self.posting_idx]
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct Table(HashMap<String, Chain>);

impl Table {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let reader = File::open(path)?;
        let table: Table = ron::de::from_reader(reader)?;
        table.validate()?;
        Ok(table)
    }

    pub fn update_transaction(&self, trn: &mut Transaction) -> Result<(), RuleError> {
        let start = self.get_chain(START_CHAIN)?;
        for i in 0..trn.postings.len() {
            let mut ctx = PostingContext {
                trn: trn,
                posting_idx: i,
            };
            start.apply(self, &mut ctx)?;
        }
        Ok(())
    }

    fn get_chain(&self, name: &str) -> Result<&Chain, RuleError> {
        self.0.get(name).ok_or_else(|| RuleError::ChainNotFound {
            chain: name.to_string(),
        })
    }

    fn validate(&self) -> Result<(), RuleError> {
        self.get_chain(START_CHAIN)?;
        for (_, chain) in &self.0 {
            chain.validate(self)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
struct Chain(Vec<Rule>);

impl Chain {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<(), RuleError> {
        for rule in &self.0 {
            match rule.apply(table, ctx)? {
                RuleResult::Continue => {}
                RuleResult::Return => break,
            }
        }
        Ok(())
    }

    fn validate(&self, table: &Table) -> Result<(), RuleError> {
        for r in &self.0 {
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
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<RuleResult, RuleError> {
        if self.predicate.is_match(ctx) {
            self.action.apply(table, ctx)?;
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
    SetAccount(String),
}

impl Action {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<(), RuleError> {
        use Action::*;

        match self {
            Noop => {}
            JumpChain(name) => {
                table.get_chain(name)?.apply(table, ctx)?;
            }
            SetAccount(v) => {
                ctx.mut_posting().account = v.clone();
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
    Account(StringMatch),
    TransactionDescription(StringMatch),
    Not(Box<Predicate>),
}

impl Predicate {
    fn is_match(&self, ctx: &PostingContext) -> bool {
        use Predicate::*;
        match self {
            True => true,
            All(preds) => preds.iter().all(|p| p.is_match(ctx)),
            Any(preds) => preds.iter().any(|p| p.is_match(ctx)),
            Account(matcher) => matcher.matches_string(&ctx.posting().account),
            TransactionDescription(matcher) => matcher.matches_string(&ctx.trn.description),
            Not(pred) => !pred.is_match(ctx),
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
    use ledger_parser::{Amount, Commodity, CommodityPosition};
    use rust_decimal::Decimal;

    use crate::builder::TransactionBuilder;

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
            self.table.0.insert(name.to_string(), chain);
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
            self.chain.0.push(Rule {
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

    fn amount(dollars: i64, cents: i64) -> Amount {
        Amount {
            commodity: Commodity {
                name: "$".to_string(),
                position: CommodityPosition::Left,
            },
            quantity: Decimal::new(dollars * 100 + cents, 2),
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
            input: Transaction,
            want: Transaction,
        }

        let test_date = NaiveDate::from_ymd(2001, 1, 2);

        let tests = vec![
            Test {
                name: "empty chain",
                table: TableBuilder::new().chain("start", Chain::default()).build(),
                cases: vec![Case {
                    input: TransactionBuilder::new(test_date, "foo").build(),
                    want: TransactionBuilder::new(test_date, "foo").build(),
                }],
            },
            Test {
                name: "set source account",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(
                                Action::SetAccount("foo".to_string()),
                                Predicate::True,
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![Case {
                    input: TransactionBuilder::new(test_date, "description")
                        .posting("anything", amount(100, 0), None)
                        .build(),
                    want: TransactionBuilder::new(test_date, "description")
                        .posting("foo", amount(100, 0), None)
                        .build(),
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
                                Action::SetAccount("foo".to_string()),
                                Predicate::True,
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![Case {
                    input: TransactionBuilder::new(test_date, "description")
                        .posting("anything", amount(100, 0), None)
                        .build(),
                    want: TransactionBuilder::new(test_date, "description")
                        .posting("foo", amount(100, 0), None)
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
                                Action::SetAccount("foo".to_string()),
                                Predicate::True,
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![Case {
                    input: TransactionBuilder::new(test_date, "description")
                        .posting("original:value", amount(100, 0), None)
                        .build(),
                    want: TransactionBuilder::new(test_date, "description")
                        .posting("original:value", amount(100, 0), None)
                        .build(),
                }],
            },
            Test {
                name: "set account based on input account",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(
                                Action::SetAccount("assets:foo".to_string()),
                                Predicate::Account(StringMatch::Eq("foo".to_string())),
                                Continue,
                            )
                            .rule(
                                Action::SetAccount("assets:bar".to_string()),
                                Predicate::Account(StringMatch::Eq("bar".to_string())),
                                Continue,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![
                    Case {
                        input: TransactionBuilder::new(test_date, "description")
                            .posting("foo", amount(100, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "description")
                            .posting("assets:foo", amount(100, 0), None)
                            .build(),
                    },
                    Case {
                        input: TransactionBuilder::new(test_date, "description")
                            .posting("bar", amount(100, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "description")
                            .posting("assets:bar", amount(100, 0), None)
                            .build(),
                    },
                    Case {
                        input: TransactionBuilder::new(test_date, "description")
                            .posting("quux", amount(100, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "description")
                            .posting("quux", amount(100, 0), None)
                            .build(),
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
                                Action::SetAccount("assets:acct1-bank1".to_string()),
                                Predicate::All(vec![
                                    Predicate::Account(StringMatch::Eq("acct1".to_string())),
                                    Predicate::TransactionDescription(StringMatch::Eq(
                                        "bank1".to_string(),
                                    )),
                                ]),
                                Return,
                            )
                            .rule(
                                Action::SetAccount("assets:acct1-bank2".to_string()),
                                Predicate::All(vec![
                                    Predicate::Account(StringMatch::Eq("acct1".to_string())),
                                    Predicate::TransactionDescription(StringMatch::Eq(
                                        "bank2".to_string(),
                                    )),
                                ]),
                                Return,
                            )
                            .rule(
                                Action::SetAccount("assets:acct2-bank1".to_string()),
                                Predicate::All(vec![
                                    Predicate::Account(StringMatch::Eq("acct2".to_string())),
                                    Predicate::TransactionDescription(StringMatch::Eq(
                                        "bank1".to_string(),
                                    )),
                                ]),
                                Return,
                            )
                            .rule(
                                Action::SetAccount("assets:acct2-bank2".to_string()),
                                Predicate::All(vec![
                                    Predicate::Account(StringMatch::Eq("acct2".to_string())),
                                    Predicate::TransactionDescription(StringMatch::Eq(
                                        "bank2".to_string(),
                                    )),
                                ]),
                                Return,
                            )
                            .rule(
                                Action::SetAccount("assets:acct3-or-4".to_string()),
                                Predicate::Any(vec![
                                    Predicate::Account(StringMatch::Eq("acct3".to_string())),
                                    Predicate::Account(StringMatch::Eq("acct4".to_string())),
                                ]),
                                Return,
                            )
                            .rule(
                                Action::SetAccount("assets:acct-other-bank1".to_string()),
                                Predicate::All(vec![
                                    Predicate::Account(StringMatch::Eq("acct1".to_string())).not(),
                                    Predicate::Account(StringMatch::Eq("acct2".to_string())).not(),
                                    Predicate::TransactionDescription(StringMatch::Eq(
                                        "bank1".to_string(),
                                    )),
                                ]),
                                Return,
                            )
                            .build(),
                    )
                    .build(),
                cases: vec![
                    Case {
                        input: TransactionBuilder::new(test_date, "unmatched")
                            .posting("acct1", amount(10, 0), None)
                            .build(),
                        // Fallthrough without matching any rules.
                        want: TransactionBuilder::new(test_date, "unmatched")
                            .posting("acct1", amount(10, 0), None)
                            .build(),
                    },
                    Case {
                        input: TransactionBuilder::new(test_date, "bank1")
                            .posting("acct1", amount(10, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "bank1")
                            .posting("assets:acct1-bank1", amount(10, 0), None)
                            .build(),
                    },
                    Case {
                        input: TransactionBuilder::new(test_date, "bank1")
                            .posting("acct2", amount(10, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "bank1")
                            .posting("assets:acct2-bank1", amount(10, 0), None)
                            .build(),
                    },
                    Case {
                        input: TransactionBuilder::new(test_date, "bank2")
                            .posting("acct2", amount(10, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "bank2")
                            .posting("assets:acct2-bank2", amount(10, 0), None)
                            .build(),
                    },
                    Case {
                        input: TransactionBuilder::new(test_date, "description")
                            .posting("acct3", amount(10, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "description")
                            .posting("assets:acct3-or-4", amount(10, 0), None)
                            .build(),
                    },
                    Case {
                        input: TransactionBuilder::new(test_date, "description")
                            .posting("acct4", amount(10, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "description")
                            .posting("assets:acct3-or-4", amount(10, 0), None)
                            .build(),
                    },
                    Case {
                        input: TransactionBuilder::new(test_date, "bank1")
                            .posting("acct5", amount(10, 0), None)
                            .build(),
                        want: TransactionBuilder::new(test_date, "bank1")
                            .posting("assets:acct-other-bank1", amount(10, 0), None)
                            .build(),
                    },
                ],
            },
        ];

        for test in &tests {
            for (i, case) in test.cases.iter().enumerate() {
                let mut trn = case.input.clone();
                test.table.update_transaction(&mut trn).unwrap();
                assert_eq!(case.want, trn, "for test {}#{}", test.name, i);
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
