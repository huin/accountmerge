/*
Transactions Transactions are movements of some quantity of commodities between
named accounts. Each transaction is represented by a journal entry beginning
with a simple date in column 0. This can be followed by any of the following,
separated by spaces:

(optional) a status character (empty, !, or *)
(optional) a transaction code (any short number or text, enclosed in parentheses)
(optional) a transaction description (any remaining text until end of line or a semicolon)
(optional) a transaction comment (any remaining text following a semicolon until end of line)
Then comes zero or more (but usually at least 2) indented lines representing…

Postings A posting is an addition of some amount to, or removal of some amount
from, an account. Each posting line begins with at least one space or tab (2 or
4 spaces is common), followed by:

(optional) a status character (empty, !, or *), followed by a space (required)
an account name (any text, optionally containing single spaces, until end of
line or a double space) (optional) two or more spaces or tabs followed by an
amount.
*/

use std::fmt;
use std::str::FromStr;

use chrono::NaiveDate;
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::space1;
use nom::combinator::{map, map_opt, map_res};
use nom::sequence::tuple;
use nom::IResult;

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

#[derive(Debug, Fail)]
enum StatusError {
    #[fail(display = "bad status string: {:?}", string)]
    InvalidStatusString { string: String },
}

#[derive(Debug, Eq, PartialEq)]
enum Status {
    Empty,
    Bang,
    Star,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        use Status::*;
        match self {
            Empty => Ok(()),
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
            "" => Ok(Empty),
            "!" => Ok(Bang),
            "*" => Ok(Star),
            _ => Err(StatusError::InvalidStatusString { string: s.into() }),
        }
    }
}

fn status(i: &str) -> IResult<&str, Status> {
    map_res(alt((tag("!"), tag("*"), tag(""))), Status::from_str)(i)
}

#[derive(Debug, Eq, PartialEq)]
struct TransactionHeader {
    date: NaiveDate,
    status: Status,
}

fn transaction_header(i: &str) -> IResult<&str, TransactionHeader> {
    map(tuple((date, space1, status)), |(date, _, status)| {
        TransactionHeader { date, status }
    })(i)
}

#[test]
fn test_transaction_header() {
    assert_eq!(
        transaction_header("2000/1/2 * TODO"),
        Ok((
            " TODO",
            TransactionHeader {
                date: NaiveDate::from_ymd(2000, 1, 2),
                status: Status::Star,
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
