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
    pub source_account: Option<String>,
    pub dest_account: Option<String>,
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
}

impl Rule {
    fn apply(
        &self,
        table: &Table,
        trn: &InputTransaction,
        cmp: &mut DerivedComponents,
    ) -> Result<RuleResult, RuleError> {
        if self.predicate.is_match(trn) {
            self.action.apply(table, trn, cmp)
        } else {
            Ok(RuleResult::Continue)
        }
    }

    fn validate(&self, table: &Table) -> Result<(), RuleError> {
        self.action.validate(table)
    }
}

enum RuleResult {
    Continue,
    Return,
}

#[derive(Debug, Deserialize)]
enum Action {
    JumpChain(String),
    Return,
    SetSrcAccount(String),
    SetDestAccount(String),
}

impl Action {
    fn apply(
        &self,
        table: &Table,
        trn: &InputTransaction,
        cmp: &mut DerivedComponents,
    ) -> Result<RuleResult, RuleError> {
        use Action::*;

        match self {
            JumpChain(name) => {
                table.get_chain(name)?.apply(table, trn, cmp)?;
            }
            Return => return Ok(RuleResult::Return),
            SetSrcAccount(v) => {
                cmp.source_account = Some(v.clone());
            }
            SetDestAccount(_) => unimplemented!(),
        }

        Ok(RuleResult::Continue)
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
    SrcBank(StringMatch),
    SrcAcct(StringMatch),
}

impl Predicate {
    fn is_match(&self, _trn: &InputTransaction) -> bool {
        use Predicate::*;
        match self {
            True => true,
            SrcBank(_) => unimplemented!(),
            SrcAcct(_) => unimplemented!(),
        }
    }
}

#[derive(Debug, Deserialize)]
enum StringMatch {
    Eq(String),
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

        fn rule(mut self, action: Action, predicate: Predicate) -> Self {
            self.chain.rules.push(Rule { action, predicate });
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
                    src_bank: "foo bank".to_string(),
                    account_name: "foo account".to_string(),
                    date: NaiveDate::from_ymd(2000, 1, 5),
                    type_: "Withdrawal".to_string(),
                    description: "".to_string(),
                    paid: Paid::In(UnsignedGbpValue::from_pence(100)),
                    balance: GbpValue::from_pence(200),
                },
            }
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
                name: "set account",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(Action::SetSrcAccount("foo".to_string()), Predicate::True)
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
                name: "set account in jumped chain",
                table: TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(jump_chain("some-chain"), Predicate::True)
                            .build(),
                    )
                    .chain(
                        "some-chain",
                        ChainBuilder::new()
                            .rule(Action::SetSrcAccount("foo".to_string()), Predicate::True)
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
                            .rule(jump_chain("foo"), Predicate::True)
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
                            .rule(jump_chain("foo"), Predicate::True)
                            .build(),
                    )
                    .chain(
                        "foo",
                        ChainBuilder::new()
                            .rule(jump_chain("not-exist"), Predicate::True)
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
