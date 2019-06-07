use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_while1};
use nom::character::complete::space1;
use nom::combinator::{map, map_opt, map_res, opt};
use nom::sequence::{pair, preceded, terminated, tuple};
use nom::{AsChar, IResult, InputTakeAtPosition};

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

/// Parses a field parsed by `field`, which must be preceded by one or more
/// spaces or tabs.
fn optional_field<I, O, F>(field: F) -> impl Fn(I) -> IResult<I, Option<O>>
where
    I: Clone,
    I: InputTakeAtPosition,
    F: Fn(I) -> IResult<I, O>,
    <I as InputTakeAtPosition>::Item: AsChar + Clone,
{
    let p = opt(map(pair(space1, field), |(_, v)| v));
    move |input: I| p(input)
}

#[derive(Debug, Fail)]
enum StatusError {
    #[fail(display = "bad status string: {:?}", string)]
    InvalidStatusString { string: String },
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
    type Err = StatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Status::*;
        match s {
            "!" => Ok(Bang),
            "*" => Ok(Star),
            _ => Err(StatusError::InvalidStatusString { string: s.into() }),
        }
    }
}

fn status(i: &str) -> IResult<&str, Status> {
    map_res(alt((tag("!"), tag("*"), tag(""))), Status::from_str)(i)
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
}

fn transaction_header(i: &str) -> IResult<&str, TransactionHeader> {
    map(
        tuple((
            date,
            optional_field(status),
            optional_field(transaction_code),
            optional_field(description),
        )),
        |(date, status, code, description)| TransactionHeader {
            date,
            status,
            code: code.map(Into::into),
            description: description.map(Into::into),
        },
    )(i)
}

#[test]
fn test_transaction_header() {
    assert_eq!(
        transaction_header("2000/1/2 * (code) description"),
        Ok((
            "",
            TransactionHeader {
                date: NaiveDate::from_ymd(2000, 1, 2),
                status: Some(Status::Star),
                code: Some("code".to_string()),
                description: Some("description".to_string()),
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
