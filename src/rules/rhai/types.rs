use std::convert::TryFrom;

use anyhow::{Error, Result};

use crate::comment::Comment;
use crate::internal::{TransactionInternal, TransactionPostings};

// Map is a newtype wrapper of `rhai::Map` to allow `From` conversions in
// both directions.
pub struct Map(pub rhai::Map);

impl From<TransactionPostings> for Map {
    fn from(trn_posts: TransactionPostings) -> Self {
        // TODO: Remaining fields.
        let mut map = rhai::Map::new();
        // pub comment: Option<String>,
        // pub date: NaiveDate,
        // pub effective_date: Option<NaiveDate>,
        // pub status: Option<TransactionStatus>,
        // pub code: Option<String>,
        // pub description: String,
        map.insert("description".into(), trn_posts.trn.raw.description.into());
        // pub postings: Vec<Posting>,
        Self(map)
    }
}

impl TryFrom<Map> for TransactionPostings {
    type Error = Error;
    fn try_from(map: Map) -> Result<Self> {
        let mut map: rhai::Map = map.0;
        // TODO: Remaining fields, reduce boilerplate.
        let description: String = map
            .remove("description")
            .ok_or_else(|| anyhow!("missing description"))?
            .try_cast()
            .ok_or_else(|| anyhow!("description was not a string"))?;
        Ok(TransactionPostings {
            trn: TransactionInternal {
                raw: ledger_parser::Transaction {
                    comment: None,
                    date: chrono::NaiveDate::from_ymd(2000, 1, 1),
                    effective_date: None,
                    status: None,
                    code: None,
                    description,
                    postings: Vec::new(),
                },
                comment: Comment::new(),
            },
            posts: Vec::new(),
        })
    }
}

impl From<Comment> for Map {
    fn from(_comment: Comment) -> Self {
        todo!("TODO: Use when converting a transaction.")
    }
}
