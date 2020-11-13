use std::collections::HashMap;
use std::io::Read;

use chrono::FixedOffset;
use failure::Error;
use regex::Regex;

/// Carries a single row from the input CSV file of timezone abbreviations.
#[derive(Debug, Deserialize)]
struct TzRecord {
    abbreviation: String,
    utc_offset: String,
}

/// Provides a mapping from timezone abbreviations to fixed UTC offsets.
pub struct TzAbbrDB {
    map: HashMap<String, FixedOffset>,
}

impl TzAbbrDB {
    pub fn from_reader<R: Read>(r: R) -> Result<Self, Error> {
        use std::collections::hash_map::Entry::*;

        let mut map = HashMap::new();
        let mut rdr = csv::Reader::from_reader(r);
        for result in rdr.deserialize() {
            let record: TzRecord = result?;
            let offset: FixedOffset = parse_utc_offset(&record.utc_offset)?;
            match map.entry(record.abbreviation) {
                Occupied(entry) => {
                    bail!(
                        "found multiple definitions of timezone abbreviation {}: {} and {}",
                        entry.key(),
                        entry.get(),
                        offset,
                    );
                }
                Vacant(entry) => {
                    entry.insert(offset);
                }
            }
        }
        Ok(Self { map })
    }

    /// Returns the fixed UTC offset for the named timezone abbreviation, if
    /// known.
    pub fn abbr_to_tz(&self, abbr: &str) -> Option<FixedOffset> {
        self.map.get(abbr).copied()
    }
}

fn parse_utc_offset(s: &str) -> Result<FixedOffset, Error> {
    lazy_static! {
        static ref UTC_HOURS_RX: Regex = Regex::new("^UTC([-+])([0-9]{2})$").unwrap();
        static ref UTC_HOURS_MINS_RX: Regex =
            Regex::new("^UTC([-+])([0-9]{2}):([0-9]{2})$").unwrap();
    }
    let (positive, hours, minutes): (bool, i32, i32) =
        if let Some(captures) = UTC_HOURS_RX.captures(s) {
            (&captures[1] == "+", captures[2].parse()?, 0)
        } else if let Some(captures) = UTC_HOURS_MINS_RX.captures(s) {
            (
                &captures[1] == "+",
                captures[2].parse()?,
                captures[3].parse()?,
            )
        } else {
            bail!("UTC offset string is not in a recognized format: {:?}", s);
        };
    if minutes > 59 {
        bail!("UTC offset minutes > 59 in {:?}", s);
    }
    let sign: i32 = if positive { 1 } else { -1 };
    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
        .ok_or_else(|| format_err!("UTC offset value is out of range in: {:?}", s))
}

#[cfg(test)]
mod tests {
    use super::{parse_utc_offset, TzAbbrDB};
    use chrono::FixedOffset;
    use failure::Error;
    use test_case::test_case;

    #[test_case("BST" => Some(FixedOffset::east(3600)))]
    #[test_case("GMT" => Some(FixedOffset::east(0)))]
    #[test_case("ZZZ" => None)]
    fn good_csv_file_lookup(abbr: &str) -> Option<FixedOffset> {
        let db = parse_string_db(
            r#"
            abbreviation,utc_offset
            BST,UTC+01
            GMT,UTC+00
        "#,
        )
        .unwrap();

        db.abbr_to_tz(abbr)
    }

    #[test_case(
        r#"
        abbreviation,utc_offset
        BST,bad-offset
    "#,
        "UTC offset string is not in a recognized format"
    )]
    // Missing field utc_offset.
    #[test_case(
        r#"
        abbreviation
        BST
    "#,
        "utc_offset"
    )]
    // Missing field abbreviation.
    #[test_case(
        r#"
        utc_offset
        UTC+01
    "#,
        "abbreviation"
    )]
    #[test_case(
        r#"
        abbreviation,utc_offset
        AAA,UTC+01
        BBB,UTC+02
        AAA,UTC-01
    "#,
        "multiple definitions of timezone abbreviation AAA: +01:00 and -01:00"
    )]
    fn bad_csv_file(content: &str, want_err_containing: &str) {
        match parse_string_db(content) {
            Ok(_) => panic!("expected an error"),
            Err(e) => {
                let msg = format!("{}", e);
                assert!(msg.contains(want_err_containing), "got error: {}", msg);
            }
        }
    }

    fn parse_string_db(s: &str) -> Result<TzAbbrDB, Error> {
        TzAbbrDB::from_reader(textwrap::dedent(s).as_bytes())
    }

    #[test_case("UTC+00" => FixedOffset::east(0) ; "UTC positive zero")]
    #[test_case("UTC-00" => FixedOffset::east(0) ; "UTC minus zero")]
    #[test_case("UTC+05" => FixedOffset::east(5 * 3600))]
    #[test_case("UTC-06" => FixedOffset::east(-6 * 3600))]
    #[test_case("UTC+23" => FixedOffset::east(23 * 3600) ; "UTC plus 23 hours")]
    #[test_case("UTC-23" => FixedOffset::east(-23 * 3600) ; "UTC minus 23 hours")]
    #[test_case("UTC+00:15" => FixedOffset::east(15 * 60) ; "UTC plus 15 minutes")]
    #[test_case("UTC-00:15" => FixedOffset::east(-15 * 60) ; "UTC minus 15 minutes")]
    #[test_case("UTC+05:15" => FixedOffset::east(5 * 3600 + 15 * 60) ; "UTC plus 5 hours 15 minutes")]
    #[test_case("UTC-06:15" => FixedOffset::east(-6 * 3600 - 15 * 60) ; "UTC minus 6 hours 15 minutes")]
    #[test_case("UTC+23:15" => FixedOffset::east(23 * 3600 + 15 * 60) ; "UTC plus 23 hours 15 minutes")]
    #[test_case("UTC-23:15" => FixedOffset::east(-23 * 3600 - 15 * 60) ; "UTC minus 23 hours 15 minutes")]
    fn good_utc_offset_string(s: &str) -> FixedOffset {
        parse_utc_offset(s).expect("expected to parse")
    }

    #[test_case("", "not in a recognized format" ; "empty string")]
    #[test_case("ZZZ+25", "not in a recognized format" ; "missing UTC prefix")]
    #[test_case("UTC+25", "out of range" ; "UTC plus 25 hours")]
    #[test_case("UTC-25", "out of range" ; "UTC minus 25 hours")]
    #[test_case("UTC+00:60", "minutes > 59" ; "UTC plus too many minutes")]
    #[test_case("UTC-00:60", "minutes > 59" ; "UTC minus too many minutes")]
    fn bad_utc_offset_string(s: &str, want_err_containing: &str) {
        match parse_utc_offset(s) {
            Ok(_) => panic!("expected an error"),
            Err(e) => {
                let msg = format!("{}", e);
                assert!(msg.contains(want_err_containing), "got error: {}", msg);
            }
        }
    }
}
