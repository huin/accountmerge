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

#[derive(Debug, Default)]
pub struct DerivedComponents {
    pub source_account: Option<String>,
    pub dest_account: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Table {
    chains: HashMap<String, RuleChain>,
}

impl Table {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let reader = File::open(path)?;
        // TODO: Error if no chain is named "start", and other consistency
        // checks.
        ron::de::from_reader(reader).map_err(std::convert::Into::into)
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

    fn get_chain(&self, name: &str) -> Result<&RuleChain, RuleError> {
        self.chains
            .get(name)
            .ok_or_else(|| RuleError::ChainNotFound {
                chain: name.to_string(),
            })
    }
}

#[derive(Debug, Deserialize)]
struct RuleChain {
    rules: Vec<Rule>,
}

impl RuleChain {
    fn apply(&self, trn: &InputTransaction, cmp: &mut DerivedComponents) {
        for rule in &self.rules {
            match rule.apply(trn, cmp) {
                RuleResult::Continue => {}
                RuleResult::Return => return,
            }
        }
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
