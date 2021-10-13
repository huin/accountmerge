use std::collections::{HashMap, HashSet};
use std::convert::{TryFrom, TryInto};

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
        map.insert("comment".into(), Dynamic::from(comment_map.0));
        map.insert(
            "date".into(),
            Dynamic::from(NaiveDate(trn_posts.trn.raw.date)),
        );
        map.insert(
            "effective_date".into(),
            Dynamic::from(trn_posts.trn.raw.effective_date.map(|d| NaiveDate(d))),
        );
        map.insert("status".into(), Dynamic::from(trn_posts.trn.raw.status));
        // pub status: Option<TransactionStatus>,
        map.insert("code".into(), Dynamic::from(trn_posts.trn.raw.code));
        map.insert(
            "description".into(),
            Dynamic::from(trn_posts.trn.raw.description),
        );
        // pub postings: Vec<Posting>,
        Self(map)
    }
}

impl TryFrom<Map> for TransactionPostings {
    type Error = Error;
    fn try_from(mut map: Map) -> Result<Self> {
        // TODO: Remaining fields.
        let date: NaiveDate = map.take_value("date")?;
        let eff_date: Option<NaiveDate> = map.take_value("effective_date")?;
        Ok(TransactionPostings {
            trn: TransactionInternal {
                raw: ledger_parser::Transaction {
                    comment: None,
                    date: date.unpack(),
                    effective_date: eff_date.map(NaiveDate::unpack),
                    status: map.take_value("status")?,
                    code: map.take_value("code")?,
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
