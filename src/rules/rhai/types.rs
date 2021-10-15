use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;

use anyhow::{Context, Error, Result};
use chrono::Datelike;
use rhai::plugin::*;
use rhai::{Dynamic, Engine};

use crate::comment::Comment;
use crate::internal::{PostingInternal, TransactionInternal, TransactionPostings};

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
        map.put_value(
            "postings",
            trn_posts
                .posts
                .into_iter()
                .map(Map::from)
                .map(Map::unpack)
                .map(Dynamic::from)
                .collect::<rhai::Array>(),
        );
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
                    postings: Vec::new(),
                },
                comment: map
                    .take_opt_value::<rhai::Map>("comment")?
                    .map(Map)
                    .map(Comment::try_from)
                    .transpose()
                    .with_context(|| "in comment")?
                    .unwrap_or_default(),
            },
            posts: map
                .take_value::<rhai::Array>("postings")?
                .into_iter()
                .map(|item: Dynamic| {
                    item.try_cast::<rhai::Map>()
                        .ok_or_else(|| anyhow!("expected Map in postings"))
                        .map(Map)
                        .and_then(PostingInternal::try_from)
                })
                .collect::<Result<Vec<PostingInternal>>>()
                .with_context(|| "in postings")?,
        })
    }
}

impl From<PostingInternal> for Map {
    fn from(posting: PostingInternal) -> Self {
        let mut map = Map::new();
        map.put_value("account", posting.raw.account);
        map.put_opt_value("amount", posting.raw.amount.map(Amount));
        map.put_opt_value("balance", posting.raw.balance);
        map.put_opt_value("status", posting.raw.status);
        map.put_value("comment", Map::from(posting.comment).unpack());
        map
    }
}

impl TryFrom<Map> for PostingInternal {
    type Error = Error;
    fn try_from(mut map: Map) -> Result<PostingInternal> {
        Ok(PostingInternal {
            raw: ledger_parser::Posting {
                account: map.take_value("account")?,
                amount: map.take_opt_value::<Amount>("amount")?.map(Amount::unpack),
                balance: map.take_opt_value("balance")?,
                status: map.take_opt_value::<ledger_parser::TransactionStatus>("status")?,
                comment: None,
            },
            comment: map
                .take_opt_value::<rhai::Map>("comment")?
                .map(Map)
                .map(Comment::try_from)
                .transpose()
                .with_context(|| "in comment")?
                .unwrap_or_default(),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Amount(ledger_parser::Amount);

impl Amount {
    pub fn register_type(engine: &mut Engine) {
        engine
            .register_type::<Self>()
            .register_fn("new_amount", Self::new)
            .register_fn("to_debug", |x: &mut Self| format!("{:?}", x))
            .register_get_set("quantity", Self::get_quantity, Self::set_quantity)
            .register_get_set("commodity", Self::get_commodity, Self::set_commodity);
    }

    fn new(quantity: rust_decimal::Decimal, commodity: ledger_parser::Commodity) -> Self {
        Self(ledger_parser::Amount {
            quantity,
            commodity,
        })
    }
    fn unpack(self) -> ledger_parser::Amount {
        self.0
    }

    fn get_quantity(&mut self) -> rust_decimal::Decimal {
        self.0.quantity
    }
    fn set_quantity(&mut self, quantity: rust_decimal::Decimal) {
        self.0.quantity = quantity;
    }
    fn get_commodity(&mut self) -> ledger_parser::Commodity {
        self.0.commodity.clone()
    }
    fn set_commodity(&mut self, commodity: ledger_parser::Commodity) {
        self.0.commodity = commodity;
    }
}

#[export_module]
mod balance_module {
    use ledger_parser::Balance;
    #[allow(non_upper_case_globals)]
    pub const balance_zero: Balance = Balance::Zero;
    pub fn balance_amount(amount: Amount) -> Balance {
        Balance::Amount(amount.unpack())
    }
    #[rhai_fn(global, get = "enum_type", pure)]
    pub fn get_type(balance: &mut Balance) -> String {
        use Balance::*;
        match balance {
            Zero => "balance_zero",
            Amount(_) => "balance_amount",
        }
        .to_string()
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(balance: &mut Balance) -> String {
        use Balance::*;
        match balance {
            Zero => "balance_zero".to_string(),
            Amount(amt) => format!("balance_amount({:?})", amt),
        }
    }
    #[rhai_fn(global, name = "==", pure)]
    pub fn eq(a: &mut Balance, b: Balance) -> bool {
        a == &b
    }
    #[rhai_fn(global, name = "!=", pure)]
    pub fn neq(a: &mut Balance, b: Balance) -> bool {
        a != &b
    }

    #[rhai_fn(global, get = "field_0", pure)]
    pub fn get_field_0(balance: &mut Balance) -> Dynamic {
        use ledger_parser::Balance::*;
        match balance {
            Zero => Dynamic::UNIT,
            Amount(amt) => Dynamic::from(Amount(amt.clone())),
        }
    }
}

#[export_module]
mod transaction_status_module {
    use ledger_parser::TransactionStatus;
    #[allow(non_upper_case_globals)]
    pub const StatusCleared: TransactionStatus = TransactionStatus::Cleared;
    #[allow(non_upper_case_globals)]
    pub const StatusPending: TransactionStatus = TransactionStatus::Pending;

    #[rhai_fn(global, get = "enum_type", pure)]
    pub fn get_type(trn_status: &mut TransactionStatus) -> String {
        match trn_status {
            TransactionStatus::Cleared => "StatusCleared",
            TransactionStatus::Pending => "StatusPending",
        }
        .to_string()
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(trn_status: &mut TransactionStatus) -> String {
        get_type(trn_status)
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
        .register_static_module("Balance", exported_module!(balance_module).into())
        .register_static_module(
            "TransactionStatus",
            exported_module!(transaction_status_module).into(),
        );
    Amount::register_type(engine);
    NaiveDate::register_type(engine);
}
