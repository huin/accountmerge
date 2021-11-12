use std::path::PathBuf;

use anyhow::{Context, Result};
use rhai::{Engine, AST};
use structopt::StructOpt;

use crate::internal::TransactionPostings;
use crate::rules::processor::{TransactionProcessor, TransactionProcessorFactory};

mod types;

#[derive(Debug, StructOpt)]
pub struct Command {
    /// The `.rhai` file containing code to change the transactions.
    rules: PathBuf,
}

impl TransactionProcessorFactory for Command {
    fn make_processor(&self) -> Result<Box<dyn TransactionProcessor>> {
        Ok(Box::new(Rhai::from_file(&self.rules)?))
    }
}

pub struct Rhai {
    engine: Engine,
    ast: AST,
}

impl Rhai {
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let mut engine = Engine::new();
        types::register_types(&mut engine);

        let ast = engine.compile_file(path.into())?;

        Ok(Rhai { engine, ast })
    }
}

impl TransactionProcessor for Rhai {
    fn update_transactions(
        &self,
        trns: Vec<TransactionPostings>,
    ) -> Result<Vec<TransactionPostings>> {
        let mut scope = rhai::Scope::new();
        trns.into_iter()
            .map(|trn| {
                let new_trn: TransactionPostings = self
                    .engine
                    .call_fn(&mut scope, &self.ast, "update_transaction", (trn,))
                    .with_context(|| "calling update_transaction()".to_string())?;
                Ok(new_trn)
            })
            .collect()
    }
}
