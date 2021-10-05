use std::collections::{HashMap, HashSet};
use std::convert::{TryFrom, TryInto};

use anyhow::{Error, Result};

use crate::comment::Comment;
use crate::internal::{TransactionInternal, TransactionPostings};

// Map is a newtype wrapper of `rhai::Map` to allow `From` conversions in
// both directions.
pub struct Map(pub rhai::Map);

impl Map {
    fn take_value<T: std::any::Any>(&mut self, key: &str) -> Result<T> {
        self.0
            .remove(key)
            .ok_or_else(|| anyhow!("missing {} field", key))?
            .try_cast()
            .ok_or_else(|| anyhow!("{} field was not the expected type", key))
    }
}

impl From<TransactionPostings> for Map {
    fn from(trn_posts: TransactionPostings) -> Self {
        // TODO: Remaining fields.
        let mut map = rhai::Map::new();
        let comment_map: Map = trn_posts.trn.comment.into();
        map.insert("comment".into(), comment_map.0.into());
        // pub date: NaiveDate,
        // pub effective_date: Option<NaiveDate>,
        // pub status: Option<TransactionStatus>,
        // pub code: Option<String>,
        map.insert("description".into(), trn_posts.trn.raw.description.into());
        // pub postings: Vec<Posting>,
        Self(map)
    }
}

impl TryFrom<Map> for TransactionPostings {
    type Error = Error;
    fn try_from(mut map: Map) -> Result<Self> {
        // TODO: Remaining fields.
        Ok(TransactionPostings {
            trn: TransactionInternal {
                raw: ledger_parser::Transaction {
                    comment: None,
                    date: chrono::NaiveDate::from_ymd(2000, 1, 1),
                    effective_date: None,
                    status: None,
                    code: None,
                    description: map.take_value("description")?,
                    postings: Vec::new(),
                },
                comment: Map(map.take_value("comment")?).try_into()?,
            },
            posts: Vec::new(),
        })
    }
}

impl From<Comment> for Map {
    fn from(comment: Comment) -> Self {
        let mut map = rhai::Map::new();
        let lines: rhai::Array = comment.lines.into_iter().map(Into::into).collect();
        let tags: rhai::Array = comment.tags.into_iter().map(Into::into).collect();
        let value_tags: rhai::Map = comment
            .value_tags
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        map.insert("lines".into(), lines.into());
        map.insert("tags".into(), tags.into());
        map.insert("value_tags".into(), value_tags.into());
        Map(map)
    }
}

impl TryFrom<Map> for Comment {
    type Error = Error;
    fn try_from(mut map: Map) -> Result<Self> {
        let lines: rhai::Array = map.take_value("lines")?;
        let tags: rhai::Array = map.take_value("tags")?;
        let value_tags: rhai::Map = map.take_value("value_tags")?;
        let comment = Comment {
            lines: lines
                .into_iter()
                .map(rhai::Dynamic::try_cast)
                .map(|opt| opt.ok_or_else(|| anyhow!("got non-string in lines array")))
                .collect::<Result<Vec<String>>>()?,
            tags: tags
                .into_iter()
                .map(rhai::Dynamic::try_cast)
                .map(|opt| opt.ok_or_else(|| anyhow!("got non-string in lines array")))
                .collect::<Result<HashSet<String>>>()?,
            value_tags: value_tags
                .into_iter()
                .map(|(key, value)| {
                    let v2 = value
                        .try_cast()
                        .ok_or_else(|| anyhow!("got non-string value in value_tags[{:?}]", key))?;
                    Ok((key.into(), v2))
                })
                .collect::<Result<HashMap<String, String>>>()?,
        };
        Ok(comment)
    }
}
