use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_while, take_while1};
use nom::character::complete::{line_ending, space0, space1};
use nom::combinator::{map, map_opt, map_res, opt};
use nom::error::ErrorKind;
use nom::sequence::{delimited, preceded, terminated, tuple};
use nom::{AsChar, IResult, InputTakeAtPosition};

use crate::money::GbpValue;

#[derive(Debug, Eq, Fail, PartialEq)]
enum ParseError {
    #[fail(display = "bad status string: {:?}", string)]
    InvalidStatusString { string: String },
}

fn account_name(i: &str) -> IResult<&str, &str> {
    let mut end: Option<usize> = None;
    {
        let mut space_pos: Option<usize> = None;
        for (pos, c) in i.char_indices() {
            match (c, space_pos) {
                ('\n', _) => {
                    end = Some(pos);
                    break;
                }
                (' ', Some(last_space_pos)) => {
                    end = Some(last_space_pos);
                    break;
                }
                (' ', None) => {
                    space_pos = Some(pos);
                }
                _ => {
                    space_pos = None;
                }
            }
        }
    }
    let end = end.ok_or(nom::Err::Error((i, ErrorKind::Complete)))?;
    let (name, remaining) = i.split_at(end);
    Ok((remaining, name))
}

#[test]
fn test_account_name() {
    assert_eq!(account_name("foo\n"), Ok(("\n", "foo")));
    assert_eq!(account_name("foo  bar\n"), Ok(("  bar\n", "foo")));
    assert_eq!(account_name("foo quux  bar\n"), Ok(("  bar\n", "foo quux")));
}

fn comment(i: &str) -> IResult<&str, &str> {
    preceded(tag(";"), take_while(|chr| chr != '\n' && chr != '\r'))(i)
}

fn date(i: &str) -> IResult<&str, NaiveDate> {
    use num::*;
    map_opt(
        alt((
            tuple((int32, tag("/"), uint32, tag("/"), uint32)),
            tuple((int32, tag("-"), uint32, tag("-"), uint32)),
            tuple((int32, tag("."), uint32, tag("."), uint32)),
        )),
        |(y, _, m, _, d)| NaiveDate::from_ymd_opt(y, m, d),
    )(i)
}

#[test]
fn test_date() {
    assert_eq!(date("2000/1/2"), Ok(("", NaiveDate::from_ymd(2000, 1, 2))));
    assert_eq!(date("2000-1-2"), Ok(("", NaiveDate::from_ymd(2000, 1, 2))));
    assert_eq!(date("2000.1.2"), Ok(("", NaiveDate::from_ymd(2000, 1, 2))));
}

fn description(i: &str) -> IResult<&str, &str> {
    map(
        take_while1(|chr| chr != ';' && chr != '\n' && chr != '\r'),
        |d: &str| d.trim_end(),
    )(i)
}

fn gbp_value(i: &str) -> IResult<&str, GbpValue> {
    map(
        tuple((tag("GBP "), opt(tag("-")), num::int32, tag("."), num::int32)),
        |(_, opt_minus, pounds, _, pence)| {
            let v = GbpValue::from_parts(pounds, pence);
            if opt_minus.is_some() {
                -v
            } else {
                v
            }
        },
    )(i)
}

/// Parses a field parsed by `field`, which must be preceded by one or more
/// spaces or tabs.
fn optional_field<I, O, F>(field: F) -> impl Fn(I) -> IResult<I, Option<O>>
where
    I: Clone,
    I: InputTakeAtPosition,
    F: Fn(I) -> IResult<I, O>,
    <I as InputTakeAtPosition>::Item: AsChar + Clone,
{
    opt(preceded(space1, field))
}

#[derive(Debug, Eq, PartialEq)]
struct Posting {
    status: Option<Status>,
    account: String,
    // TODO: Support other currencies and formats.
    amount: Option<GbpValue>,
    // TODO: Balance assertion.
}

fn posting(i: &str) -> IResult<&str, Posting> {
    map(
        delimited(
            space1,
            tuple((
                opt(terminated(status, space1)),
                account_name,
                opt(preceded(tag("  "), preceded(space0, gbp_value))),
            )),
            line_ending,
        ),
        |(opt_status, account, opt_amount)| Posting {
            status: opt_status,
            account: account.to_string(),
            amount: opt_amount,
        },
    )(i)
}

#[test]
fn test_posting() {
    assert_eq!(
        posting("  account name\n"),
        Ok((
            "",
            Posting {
                status: None,
                account: "account name".to_string(),
                amount: None,
            }
        ))
    );
    assert_eq!(
        posting("  account name  GBP 100.00\n"),
        Ok((
            "",
            Posting {
                status: None,
                account: "account name".to_string(),
                amount: Some(GbpValue::from_parts(100, 0)),
            }
        ))
    );
    assert_eq!(
        posting("  * account name  GBP 100.00\n"),
        Ok((
            "",
            Posting {
                status: Some(Status::Star),
                account: "account name".to_string(),
                amount: Some(GbpValue::from_parts(100, 0)),
            }
        ))
    );
}

#[derive(Debug, Eq, PartialEq)]
enum Status {
    Bang,
    Star,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use Status::*;
        match self {
            Bang => f.write_str("!"),
            Star => f.write_str("*"),
        }
    }
}

impl FromStr for Status {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Status::*;
        match s {
            "!" => Ok(Bang),
            "*" => Ok(Star),
            _ => Err(ParseError::InvalidStatusString { string: s.into() }),
        }
    }
}

fn status(i: &str) -> IResult<&str, Status> {
    map_res(alt((tag("!"), tag("*"))), Status::from_str)(i)
}

fn transaction_code(i: &str) -> IResult<&str, &str> {
    terminated(
        preceded(
            tag("("),
            take_while1(|chr| chr != ')' && chr != '\n' && chr != '\r'),
        ),
        tag(")"),
    )(i)
}

#[derive(Debug, Eq, PartialEq)]
struct TransactionHeader {
    date: NaiveDate,
    status: Option<Status>,
    code: Option<String>,
    description: Option<String>,
    comment: Option<String>,
}

fn transaction_header(i: &str) -> IResult<&str, TransactionHeader> {
    map(
        tuple((
            date,
            optional_field(status),
            optional_field(transaction_code),
            optional_field(description),
            opt(comment),
            line_ending,
        )),
        |(date, status, code, description, comment, _)| TransactionHeader {
            date,
            status,
            code: code.map(Into::into),
            description: description.map(Into::into),
            comment: comment.map(Into::into),
        },
    )(i)
}

#[test]
fn test_transaction_header() {
    assert_eq!(
        transaction_header("2000/1/2 * (code) description\n"),
        Ok((
            "",
            TransactionHeader {
                date: NaiveDate::from_ymd(2000, 1, 2),
                status: Some(Status::Star),
                code: Some("code".to_string()),
                description: Some("description".to_string()),
                comment: None,
            }
        ))
    );
}

mod num {
    use std::str::FromStr;

    use nom::character::complete::digit1;
    use nom::combinator::map_res;
    use nom::IResult;

    pub fn int32(i: &str) -> IResult<&str, i32> {
        map_res(digit1, i32::from_str)(i)
    }

    pub fn uint32(i: &str) -> IResult<&str, u32> {
        map_res(digit1, u32::from_str)(i)
    }

    #[test]
    fn tests() {
        assert_eq!(int32("1234"), Ok(("", 1234)));
        assert_eq!(uint32("1234"), Ok(("", 1234)));
    }
}
