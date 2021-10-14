use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use anyhow::{Error, Result};
use chrono::Datelike;
use rhai::plugin::*;
use rhai::{Dynamic, Engine};

use crate::comment::Comment;
use crate::internal::{TransactionInternal, TransactionPostings};

// Map is a newtype wrapper of `rhai::Map` to allow `From` conversions in
// both directions.
pub struct Map(pub rhai::Map);

impl Map {
    fn new() -> Self {
        Map(rhai::Map::new())
    }

    fn unpack(self) -> rhai::Map {
        self.0
    }

    fn take_value<T: Any>(&mut self, key: &str) -> Result<T> {
        self.0
            .remove(key)
            .ok_or_else(|| anyhow!("missing {} field", key))?
            .try_cast()
            .ok_or_else(|| anyhow!("{} field was not the expected type", key))
    }

    fn take_opt_value<T: Any>(&mut self, key: &str) -> Result<Option<T>> {
        let value: Dynamic = match self.0.remove(key) {
            Some(v) => v,
            None => return Ok(None),
        };
        if value.is::<()>() {
            return Ok(None);
        }
        value
            .try_cast::<T>()
            .ok_or_else(|| anyhow!("{} field was not the expected type", key))
            .map(Some)
    }

    fn put_value<T: Any + Clone + Send + Sync>(&mut self, key: &str, value: T) {
        self.0.insert(key.into(), Dynamic::from(value));
    }

    fn put_opt_value<T: Any + Clone + Send + Sync>(&mut self, key: &str, value: Option<T>) {
        self.0.insert(
            key.into(),
            match value {
                None => Dynamic::from(()),
                Some(value) => Dynamic::from(value),
            },
        );
    }
}

impl From<TransactionPostings> for Map {
    fn from(trn_posts: TransactionPostings) -> Self {
        let mut map = Self::new();
        map.put_value("comment", Map::from(trn_posts.trn.comment).unpack());
        map.put_value("date", NaiveDate(trn_posts.trn.raw.date));
        map.put_opt_value(
            "effective_date",
            trn_posts.trn.raw.effective_date.map(NaiveDate),
        );
        map.put_opt_value("status", trn_posts.trn.raw.status);
        map.put_opt_value("code", trn_posts.trn.raw.code);
        map.put_value("description", trn_posts.trn.raw.description);
        // TODO: Postings.
        // pub postings: Vec<Posting>,
        map
    }
}

impl TryFrom<Map> for TransactionPostings {
    type Error = Error;
    fn try_from(mut map: Map) -> Result<Self> {
        Ok(TransactionPostings {
            trn: TransactionInternal {
                raw: ledger_parser::Transaction {
                    comment: None,
                    date: map.take_value::<NaiveDate>("date")?.unpack(),
                    effective_date: map
                        .take_opt_value::<NaiveDate>("effective_date")?
                        .map(NaiveDate::unpack),
                    status: map.take_opt_value("status")?,
                    code: map.take_opt_value("code")?,
                    description: map.take_value("description")?,
                    // TODO: Postings.
                    postings: Vec::new(),
                },
                comment: map
                    .take_opt_value::<rhai::Map>("comment")?
                    .map(Map)
                    .map(Comment::try_from)
                    .transpose()?
                    .unwrap_or_default(),
            },
            posts: Vec::new(),
        })
    }
}

impl From<Comment> for Map {
    fn from(comment: Comment) -> Self {
        let mut map = rhai::Map::new();
        let lines: rhai::Array = comment.lines.into_iter().map(Dynamic::from).collect();
        let tags: rhai::Array = comment.tags.into_iter().map(Dynamic::from).collect();
        let value_tags: rhai::Map = comment
            .value_tags
            .into_iter()
            .map(|(key, value)| (key.into(), Dynamic::from(value)))
            .collect();
        map.insert("lines".into(), Dynamic::from(lines));
        map.insert("tags".into(), Dynamic::from(tags));
        map.insert("value_tags".into(), Dynamic::from(value_tags));
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

#[export_module]
mod transaction_status_module {
    use ledger_parser::TransactionStatus;
    #[allow(non_upper_case_globals)]
    pub const Cleared: TransactionStatus = TransactionStatus::Cleared;
    #[allow(non_upper_case_globals)]
    pub const Pending: TransactionStatus = TransactionStatus::Pending;

    #[rhai_fn(global, get = "enum_type", pure)]
    pub fn get_type(trn_status: &mut TransactionStatus) -> String {
        match trn_status {
            TransactionStatus::Cleared => "Cleared",
            TransactionStatus::Pending => "Pending",
        }
        .to_string()
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(trn_status: &mut TransactionStatus) -> String {
        format!("{:?}", trn_status)
    }
    #[rhai_fn(global, name = "==", pure)]
    pub fn eq(a: &mut TransactionStatus, b: TransactionStatus) -> bool {
        a == &b
    }
    #[rhai_fn(global, name = "!=", pure)]
    pub fn neq(a: &mut TransactionStatus, b: TransactionStatus) -> bool {
        a != &b
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NaiveDate(pub chrono::NaiveDate);

impl NaiveDate {
    pub fn register_type(engine: &mut Engine) {
        engine
            .register_type::<Self>()
            .register_fn("new_date", Self::new)
            .register_fn("to_debug", |x: &mut Self| format!("{:?}", x))
            .register_get_set("year", Self::get_year, Self::set_year)
            .register_get_set("month", Self::get_month, Self::set_month)
            .register_get_set("day", Self::get_day, Self::set_day);
    }

    fn new(year: i32, month: u32, day: u32) -> Self {
        Self(chrono::NaiveDate::from_ymd(year, month, day))
    }

    fn unpack(self) -> chrono::NaiveDate {
        self.0
    }

    fn get_year(&mut self) -> i64 {
        self.0.year() as i64
    }
    fn get_month(&mut self) -> i64 {
        self.0.month() as i64
    }
    fn get_day(&mut self) -> i64 {
        self.0.day() as i64
    }
    fn set_year(&mut self, year: i64) {
        self.0 = chrono::NaiveDate::from_ymd(year as i32, self.0.month(), self.0.day())
    }
    fn set_month(&mut self, month: i64) {
        self.0 = chrono::NaiveDate::from_ymd(self.0.year(), month as u32, self.0.day())
    }
    fn set_day(&mut self, day: i64) {
        self.0 = chrono::NaiveDate::from_ymd(self.0.year(), self.0.month(), day as u32)
    }
}

pub fn register_types(engine: &mut Engine) {
    engine
        .register_type_with_name::<ledger_parser::TransactionStatus>("TransactionStatus")
        .register_static_module(
            "TransactionStatus",
            exported_module!(transaction_status_module).into(),
        );
    NaiveDate::register_type(engine);
}
