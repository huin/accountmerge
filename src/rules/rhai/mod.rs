use std::convert::TryInto;

use anyhow::Result;
use rhai::{Engine, AST};

use crate::internal::TransactionPostings;
use crate::rules::processor::TransactionProcessor;

mod types;

pub struct Rhai {
    engine: Engine,
    ast: AST,
}

impl Rhai {
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let engine = Engine::new();
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
                let trn_obj: types::Map = trn.into();
                let result: rhai::Map = self.engine.call_fn(
                    &mut scope,
                    &self.ast,
                    "update_transaction",
                    (trn_obj.0,),
                )?;
                let new_trn: TransactionPostings = types::Map(result).try_into()?;
                Ok(new_trn)
            })
            .collect()
    }
}
