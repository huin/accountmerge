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
        start.apply(trn, &mut cmp);
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
    fn apply(&self, trn: &InputTransaction, cmp: &mut DerivedComponents) {
        for rule in &self.rules {
            match rule.apply(trn, cmp) {
                RuleResult::Continue => {}
                RuleResult::Return => return,
            }
        }
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
    fn apply(&self, trn: &InputTransaction, cmp: &mut DerivedComponents) -> RuleResult {
        if self.predicate.is_match(trn) {
            self.action.apply(trn, cmp)
        } else {
            RuleResult::Continue
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
    fn apply(&self, _trn: &InputTransaction, cmp: &mut DerivedComponents) -> RuleResult {
        use Action::*;

        match self {
            JumpChain(_) => unimplemented!(),
            Return => return RuleResult::Return,
            SetSrcAccount(v) => {
                cmp.source_account = Some(v.clone());
            }
            SetDestAccount(_) => unimplemented!(),
        }

        RuleResult::Continue
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

    #[test]
    fn apply() {
        struct Test {
            name: &'static str,
            table: Table,
            trn: InputTransaction,
            want: DerivedComponents,
        };
        let tests = vec![Test {
            name: "empty chain",
            table: TableBuilder::new().chain("start", Chain::default()).build(),
            trn: InputTransaction {
                src_bank: "foo bank".to_string(),
                account_name: "foo account".to_string(),
                date: NaiveDate::from_ymd(2000, 1, 5),
                type_: "Withdrawal".to_string(),
                description: "".to_string(),
                paid: Paid::In(UnsignedGbpValue::from_pence(100)),
                balance: GbpValue::from_pence(200),
            },
            want: DerivedComponents {
                source_account: None,
                dest_account: None,
            },
        }];

        for t in &tests {
            let cmp = t.table.derive_components(&t.trn).unwrap();
            assert_eq!(t.want, cmp, "for test {}", t.name);
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
                            .rule(Action::JumpChain("other".to_string()), Predicate::True)
                            .build(),
                    )
                    .chain("other", Chain::default())
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
                            .rule(Action::JumpChain("foo".to_string()), Predicate::True)
                            .build(),
                    )
                    .chain(
                        "other",
                        ChainBuilder::new()
                            .rule(Action::JumpChain("not exist".to_string()), Predicate::True)
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
