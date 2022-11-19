use chrono::NaiveDate;
use rhai::plugin::*;
use rhai::{Dynamic, Engine};

use crate::comment::Comment;

type RawResult<T> = std::result::Result<T, Box<EvalAltResult>>;

fn bad_type(want_type: &str) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorMismatchDataType(
        want_type.into(),
        "<unknown>".into(),
        Position::NONE,
    ))
}

fn opt_clone_to_dynamic<T: 'static + Clone + Send + Sync>(value: &Option<T>) -> Dynamic {
    value.clone().map(Dynamic::from).unwrap_or(Dynamic::UNIT)
}

#[export_module]
mod amount_module {
    use ledger_parser::{Amount, Commodity};
    use rust_decimal::Decimal;

    pub fn create(quantity: rust_decimal::Decimal, commodity: ledger_parser::Commodity) -> Amount {
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

    pub fn create() -> Comment {
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
    pub fn set_lines(comment: &mut Comment, lines: rhai::Array) -> RawResult<()> {
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

    pub fn create(name: String, position: CommodityPosition) -> Commodity {
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
        commodity.position
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
mod date_module {
    use chrono::{Datelike, NaiveDate};

    pub fn create(year: i32, month: u32, day: u32) -> Dynamic {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .map(Dynamic::from)
            .unwrap_or_else(|| ().into())
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

    #[rhai_fn(get = "month", pure)]
    pub fn get_month(date: &mut NaiveDate) -> i64 {
        date.month() as i64
    }

    #[rhai_fn(get = "day", pure)]
    pub fn get_day(date: &mut NaiveDate) -> i64 {
        date.day() as i64
    }
}

#[export_module]
mod posting_module {
    use ledger_parser::{Balance, Posting, PostingAmount, Reality, TransactionStatus};
    use rhai::Dynamic;

    use crate::comment::Comment;
    use crate::internal::PostingInternal;

    pub fn create(account: String) -> PostingInternal {
        PostingInternal {
            comment: Comment::new(),
            raw: Posting {
                account,
                reality: Reality::Real,
                amount: None,
                balance: None,
                status: None,
                comment: None,
            },
        }
    }

    #[rhai_fn(global, name = "to_string", name = "to_debug", pure)]
    pub fn to_string(posting: &mut PostingInternal) -> String {
        format!("{:?}", posting)
    }

    #[rhai_fn(get = "account", pure)]
    pub fn get_account(posting: &mut PostingInternal) -> String {
        posting.raw.account.clone()
    }
    #[rhai_fn(set = "account")]
    pub fn set_account(posting: &mut PostingInternal, account: String) {
        posting.raw.account = account;
    }

    #[rhai_fn(get = "amount", pure)]
    pub fn get_amount(posting: &mut PostingInternal) -> Dynamic {
        opt_clone_to_dynamic(&posting.raw.amount)
    }
    #[rhai_fn(set = "amount")]
    pub fn set_amount(posting: &mut PostingInternal, amount: PostingAmount) {
        posting.raw.amount = Some(amount);
    }
    #[rhai_fn(set = "amount")]
    pub fn set_amount_none(posting: &mut PostingInternal, _: ()) {
        posting.raw.amount = None;
    }

    #[rhai_fn(get = "balance", pure)]
    pub fn get_balance(posting: &mut PostingInternal) -> Dynamic {
        opt_clone_to_dynamic(&posting.raw.balance)
    }
    #[rhai_fn(set = "balance")]
    pub fn set_balance(posting: &mut PostingInternal, balance: Balance) {
        posting.raw.balance = Some(balance);
    }
    #[rhai_fn(set = "balance")]
    pub fn set_balance_none(posting: &mut PostingInternal, _: ()) {
        posting.raw.balance = None;
    }

    #[rhai_fn(get = "status", pure)]
    pub fn get_status(posting: &mut PostingInternal) -> Dynamic {
        opt_clone_to_dynamic(&posting.raw.status)
    }
    #[rhai_fn(set = "status")]
    pub fn set_status(posting: &mut PostingInternal, status: TransactionStatus) {
        posting.raw.status = Some(status);
    }
    #[rhai_fn(set = "status")]
    pub fn set_status_none(posting: &mut PostingInternal, _: ()) {
        posting.raw.status = None;
    }

    #[rhai_fn(global, get = "comment", pure)]
    pub fn get_comment(posting: &mut PostingInternal) -> Comment {
        posting.comment.clone()
    }
    #[rhai_fn(global, set = "comment")]
    pub fn set_comment(posting: &mut PostingInternal, comment: Comment) {
        posting.comment = comment;
    }
}

#[export_module]
mod transaction_module {
    use ledger_parser::{Transaction, TransactionStatus};
    use rhai::Array;

    use crate::comment::Comment;
    use crate::internal::{PostingInternal, TransactionInternal, TransactionPostings};

    pub fn create(date: NaiveDate, description: String) -> TransactionPostings {
        TransactionPostings {
            posts: Vec::new(),
            trn: TransactionInternal {
                raw: Transaction {
                    comment: None,
                    date,
                    effective_date: None,
                    status: None,
                    code: None,
                    description,
                    postings: Vec::new(),
                },
                comment: Comment::new(),
            },
        }
    }

    #[rhai_fn(get = "comment", pure)]
    pub fn get_comment(trn: &mut TransactionPostings) -> Comment {
        trn.trn.comment.clone()
    }
    #[rhai_fn(set = "comment")]
    pub fn set_comment(trn: &mut TransactionPostings, comment: Comment) {
        trn.trn.comment = comment;
    }

    #[rhai_fn(get = "date", pure)]
    pub fn get_date(trn: &mut TransactionPostings) -> NaiveDate {
        trn.trn.raw.date
    }
    #[rhai_fn(set = "date")]
    pub fn set_date(trn: &mut TransactionPostings, date: NaiveDate) {
        trn.trn.raw.date = date;
    }

    #[rhai_fn(get = "effective_date", pure)]
    pub fn get_effective_date(trn: &mut TransactionPostings) -> Dynamic {
        opt_clone_to_dynamic(&trn.trn.raw.effective_date)
    }
    #[rhai_fn(set = "effective_date")]
    pub fn set_effective_date(trn: &mut TransactionPostings, effective_date: NaiveDate) {
        trn.trn.raw.effective_date = Some(effective_date);
    }
    #[rhai_fn(set = "effective_date")]
    pub fn set_effective_date_none(trn: &mut TransactionPostings, _: ()) {
        trn.trn.raw.effective_date = None;
    }

    #[rhai_fn(get = "status", pure)]
    pub fn get_status(trn: &mut TransactionPostings) -> Dynamic {
        opt_clone_to_dynamic(&trn.trn.raw.status)
    }
    #[rhai_fn(set = "status")]
    pub fn set_status(trn: &mut TransactionPostings, status: TransactionStatus) {
        trn.trn.raw.status = Some(status);
    }
    #[rhai_fn(set = "status")]
    pub fn set_status_none(trn: &mut TransactionPostings, _: ()) {
        trn.trn.raw.status = None;
    }

    #[rhai_fn(get = "code", pure)]
    pub fn get_code(trn: &mut TransactionPostings) -> Dynamic {
        opt_clone_to_dynamic(&trn.trn.raw.code)
    }
    #[rhai_fn(set = "code")]
    pub fn set_code(trn: &mut TransactionPostings, code: String) {
        trn.trn.raw.code = Some(code);
    }
    #[rhai_fn(set = "code")]
    pub fn set_code_none(trn: &mut TransactionPostings, _: ()) {
        trn.trn.raw.code = None;
    }

    #[rhai_fn(get = "description", pure)]
    pub fn get_description(trn: &mut TransactionPostings) -> String {
        trn.trn.raw.description.clone()
    }
    #[rhai_fn(set = "description")]
    pub fn set_description(trn: &mut TransactionPostings, description: String) {
        trn.trn.raw.description = description;
    }

    #[rhai_fn(get = "postings", pure)]
    pub fn get_postings(trn: &mut TransactionPostings) -> Array {
        trn.posts
            .iter()
            .cloned()
            .map(Dynamic::from)
            .collect::<rhai::Array>()
    }
    #[rhai_fn(set = "postings", return_raw)]
    pub fn set_postings(trn: &mut TransactionPostings, postings: Array) -> RawResult<()> {
        trn.posts = postings
            .into_iter()
            .map(rhai::Dynamic::try_cast)
            .map(|opt: Option<PostingInternal>| opt.ok_or_else(|| bad_type("Posting")))
            .collect::<std::result::Result<Vec<PostingInternal>, Box<EvalAltResult>>>()?;
        Ok(())
    }
}

#[export_module]
mod transaction_status_module {
    use ledger_parser::TransactionStatus;
    #[allow(non_upper_case_globals)]
    pub const Cleared: TransactionStatus = TransactionStatus::Cleared;
    #[allow(non_upper_case_globals)]
    pub const Pending: TransactionStatus = TransactionStatus::Pending;

    #[rhai_fn(get = "enum_type", pure)]
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
        .register_static_module("Posting", exported_module!(posting_module).into())
        .register_static_module("Transaction", exported_module!(transaction_module).into())
        .register_static_module(
            "TransactionStatus",
            exported_module!(transaction_status_module).into(),
        );
}
