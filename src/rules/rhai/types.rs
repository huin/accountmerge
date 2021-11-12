use std::any::Any;
use std::convert::TryFrom;

use anyhow::{Context, Error, Result};
use chrono::NaiveDate;
use rhai::plugin::*;
use rhai::{Dynamic, Engine};

use crate::comment::Comment;
use crate::internal::{PostingInternal, TransactionInternal, TransactionPostings};

type RawResult<T> = std::result::Result<T, Box<EvalAltResult>>;

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
        map.put_value("comment", trn_posts.trn.comment);
        map.put_value("date", trn_posts.trn.raw.date);
        map.put_opt_value("effective_date", trn_posts.trn.raw.effective_date);
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

fn bad_type(want_type: &str) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorMismatchDataType(
        want_type.into(),
        "<unknown>".into(),
        Position::NONE,
    ))
}

impl TryFrom<Map> for TransactionPostings {
    type Error = Error;
    fn try_from(mut map: Map) -> Result<Self> {
        Ok(TransactionPostings {
            trn: TransactionInternal {
                raw: ledger_parser::Transaction {
                    comment: None,
                    date: map.take_value::<NaiveDate>("date")?,
                    effective_date: map.take_opt_value::<NaiveDate>("effective_date")?,
                    status: map.take_opt_value("status")?,
                    code: map.take_opt_value("code")?,
                    description: map.take_value("description")?,
                    postings: Vec::new(),
                },
                comment: map
                    .take_opt_value::<Comment>("comment")?
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
        map.put_opt_value("amount", posting.raw.amount);
        map.put_opt_value("balance", posting.raw.balance);
        map.put_opt_value("status", posting.raw.status);
        map.put_value("comment", posting.comment);
        map
    }
}

impl TryFrom<Map> for PostingInternal {
    type Error = Error;
    fn try_from(mut map: Map) -> Result<PostingInternal> {
        Ok(PostingInternal {
            raw: ledger_parser::Posting {
                account: map.take_value("account")?,
                amount: map.take_opt_value::<ledger_parser::Amount>("amount")?,
                balance: map.take_opt_value("balance")?,
                status: map.take_opt_value::<ledger_parser::TransactionStatus>("status")?,
                comment: None,
            },
            comment: map
                .take_opt_value::<Comment>("comment")?
                .unwrap_or_default(),
        })
    }
}

#[export_module]
mod amount_module {
    use ledger_parser::{Amount, Commodity};
    use rust_decimal::Decimal;

    pub fn new(quantity: rust_decimal::Decimal, commodity: ledger_parser::Commodity) -> Amount {
        Amount {
            quantity,
            commodity,
        }
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(amount: &mut Amount) -> String {
        format!("{:?}", amount)
    }

    #[rhai_fn(get = "quantity", pure)]
    pub fn get_quantity(amount: &mut Amount) -> Decimal {
        amount.quantity
    }
    #[rhai_fn(set = "quantity")]
    pub fn set_quantity(amount: &mut Amount, quantity: Decimal) {
        amount.quantity = quantity;
    }

    #[rhai_fn(get = "commodity", pure)]
    pub fn get_commodity(amount: &mut Amount) -> Commodity {
        amount.commodity.clone()
    }
    #[rhai_fn(set = "commodity")]
    pub fn set_commodity(amount: &mut Amount, commodity: Commodity) {
        amount.commodity = commodity;
    }
}

#[export_module]
mod balance_module {
    use ledger_parser::{Amount, Balance};
    #[allow(non_upper_case_globals)]
    pub const Zero: Balance = Balance::Zero;
    #[rhai_fn(name = "Amount")]
    pub fn amount(amount: Amount) -> Balance {
        Balance::Amount(amount)
    }
    #[rhai_fn(get = "enum_type", pure)]
    pub fn get_type(balance: &mut Balance) -> String {
        use Balance::*;
        match balance {
            Zero => "Zero",
            Amount(_) => "Amount",
        }
        .to_string()
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(balance: &mut Balance) -> String {
        use Balance::*;
        match balance {
            Zero => "Balance::Zero".to_string(),
            Amount(amt) => format!("Balance::Amount({:?})", amt),
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
            Amount(amt) => Dynamic::from(amt.clone()),
        }
    }
}

#[export_module]
mod comment_module {
    use std::collections::{HashMap, HashSet};

    pub fn new() -> Comment {
        Comment::new()
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(comment: &mut Comment) -> String {
        format!("{:?}", comment)
    }

    #[rhai_fn(get = "lines", pure)]
    pub fn get_lines(comment: &mut Comment) -> rhai::Array {
        comment.lines.iter().cloned().map(Dynamic::from).collect()
    }

    #[rhai_fn(set = "lines", return_raw)]
    pub fn set_lines(comment: &mut Comment, lines: rhai::Array) -> Result<(), Box<EvalAltResult>> {
        comment.lines = lines
            .into_iter()
            .map(rhai::Dynamic::try_cast)
            .map(|opt: Option<String>| opt.ok_or_else(|| bad_type("String")))
            .collect::<std::result::Result<Vec<String>, Box<EvalAltResult>>>()?;
        Ok(())
    }

    #[rhai_fn(get = "tags", pure)]
    pub fn get_tags(comment: &mut Comment) -> rhai::Array {
        comment.tags.iter().cloned().map(Dynamic::from).collect()
    }

    #[rhai_fn(set = "tags", return_raw)]
    pub fn set_tags(comment: &mut Comment, tags: rhai::Array) -> RawResult<()> {
        comment.tags = tags
            .into_iter()
            .map(rhai::Dynamic::try_cast)
            .map(|opt: Option<String>| opt.ok_or_else(|| bad_type("String")))
            .collect::<RawResult<HashSet<String>>>()?;
        Ok(())
    }

    #[rhai_fn(get = "value_tags", pure)]
    pub fn get_value_tags(comment: &mut Comment) -> rhai::Map {
        comment
            .value_tags
            .iter()
            .map(|(key, value)| (key.into(), Dynamic::from(value.clone())))
            .collect()
    }

    #[rhai_fn(set = "value_tags", return_raw)]
    pub fn set_value_tags(comment: &mut Comment, value_tags: rhai::Map) -> RawResult<()> {
        comment.value_tags = value_tags
            .into_iter()
            .map(|(key, value)| {
                let v2 = value.try_cast().ok_or_else(|| bad_type("String"))?;
                Ok((key.into(), v2))
            })
            .collect::<RawResult<HashMap<String, String>>>()?;
        Ok(())
    }
}

#[export_module]
mod commodity_module {
    use ledger_parser::{Commodity, CommodityPosition};
    pub fn new(name: String, position: CommodityPosition) -> Commodity {
        Commodity { name, position }
    }

    #[rhai_fn(global, get = "enum_type", pure)]
    pub fn get_type(_commodity: &mut Commodity) -> String {
        "Commodity".to_string()
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(commodity: &mut Commodity) -> String {
        format!("{:?}", commodity)
    }
    #[rhai_fn(global, name = "==", pure)]
    pub fn eq(a: &mut Commodity, b: Commodity) -> bool {
        a == &b
    }
    #[rhai_fn(global, name = "!=", pure)]
    pub fn neq(a: &mut Commodity, b: Commodity) -> bool {
        a != &b
    }

    #[rhai_fn(global, get = "name", pure)]
    pub fn get_name(commodity: &mut Commodity) -> String {
        commodity.name.clone()
    }
    #[rhai_fn(global, set = "name")]
    pub fn set_name(commodity: &mut Commodity, name: String) {
        commodity.name = name
    }

    #[rhai_fn(global, get = "position", pure)]
    pub fn get_position(commodity: &mut Commodity) -> CommodityPosition {
        commodity.position.clone()
    }
    #[rhai_fn(global, set = "position")]
    pub fn set_position(commodity: &mut Commodity, position: CommodityPosition) {
        commodity.position = position
    }
}

#[export_module]
mod commodity_position_module {
    use ledger_parser::CommodityPosition;
    #[allow(non_upper_case_globals)]
    pub const Left: CommodityPosition = CommodityPosition::Left;
    #[allow(non_upper_case_globals)]
    pub const Right: CommodityPosition = CommodityPosition::Right;

    #[rhai_fn(global, get = "enum_type", pure)]
    pub fn get_type(position: &mut CommodityPosition) -> String {
        use CommodityPosition::*;
        match position {
            Left => "CommodityPosition::Left",
            Right => "CommodityPosition::Right",
        }
        .to_string()
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(position: &mut CommodityPosition) -> String {
        get_type(position)
    }
    #[rhai_fn(global, name = "==", pure)]
    pub fn eq(a: &mut CommodityPosition, b: CommodityPosition) -> bool {
        a == &b
    }
    #[rhai_fn(global, name = "!=", pure)]
    pub fn neq(a: &mut CommodityPosition, b: CommodityPosition) -> bool {
        a != &b
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

#[export_module]
mod date_module {
    use chrono::{Datelike, NaiveDate};

    pub fn new(year: i32, month: u32, day: u32) -> NaiveDate {
        chrono::NaiveDate::from_ymd(year, month, day)
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(date: &mut NaiveDate) -> String {
        format!(
            "Date({}-{:02}-{:02})",
            date.year(),
            date.month(),
            date.day()
        )
    }

    #[rhai_fn(get = "year", pure)]
    pub fn get_year(date: &mut NaiveDate) -> i64 {
        date.year() as i64
    }
    #[rhai_fn(set = "year")]
    pub fn set_year(date: &mut NaiveDate, year: i64) {
        *date = NaiveDate::from_ymd(year as i32, date.month(), date.day())
    }

    #[rhai_fn(get = "month", pure)]
    pub fn get_month(date: &mut NaiveDate) -> i64 {
        date.month() as i64
    }
    #[rhai_fn(set = "month")]
    pub fn set_month(date: &mut NaiveDate, month: i64) {
        *date = NaiveDate::from_ymd(date.year(), month as u32, date.day())
    }

    #[rhai_fn(get = "day", pure)]
    pub fn get_day(date: &mut NaiveDate) -> i64 {
        date.day() as i64
    }
    #[rhai_fn(set = "day")]
    pub fn set_day(date: &mut NaiveDate, day: i64) {
        *date = NaiveDate::from_ymd(date.year(), date.month(), day as u32)
    }
}

pub fn register_types(engine: &mut Engine) {
    engine
        .register_static_module("Amount", exported_module!(amount_module).into())
        .register_static_module("Balance", exported_module!(balance_module).into())
        .register_static_module("Comment", exported_module!(comment_module).into())
        .register_static_module("Commodity", exported_module!(commodity_module).into())
        .register_static_module(
            "CommodityPosition",
            exported_module!(commodity_position_module).into(),
        )
        .register_static_module("Date", exported_module!(date_module).into())
        .register_static_module(
            "TransactionStatus",
            exported_module!(transaction_status_module).into(),
        );
}
