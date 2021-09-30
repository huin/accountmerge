use anyhow::Result;
use chrono::NaiveDate;
use ledger_parser::{Amount, Balance, Posting, Transaction};
use structopt::StructOpt;

use crate::accounts::ASSETS_UNKNOWN;
use crate::comment::Comment;
use crate::filespec::FileSpec;
use crate::importers::importer::TransactionImporter;
use crate::importers::nationwide::{CommonOpts, BANK_NAME};
use crate::importers::nationwide_csv::de::*;
use crate::importers::util::{negate_amount, self_and_peer_account_amount};
use crate::tags;

/// Transaction type field, provided by the bank.
pub const TRANSACTION_TYPE_TAG: &str = "trn_type";

#[derive(Debug, Deserialize)]
struct AccountName {
    header: String,
    account_name: String,
}

#[derive(Debug, Deserialize)]
struct AccountQuantity {
    header: String,
    amount: GbpValue,
}

#[derive(Debug, StructOpt)]
/// Converts from Nationwide (nationwide.co.uk) CSV format to Ledger
/// transactions.
pub struct NationwideCsv {
    /// Nationwide CSV file to read from. "-" reads from stdin.
    input: FileSpec,

    /// Generate the legacy fingerprint tag.
    #[structopt(long = "include-legacy-fingerprint")]
    include_legacy_fingerprint: bool,

    #[structopt(flatten)]
    commonopts: CommonOpts,
}

impl TransactionImporter for NationwideCsv {
    fn get_transactions(&self) -> Result<Vec<Transaction>> {
        let reader = encoding_rs_io::DecodeReaderBytesBuilder::new()
            .encoding(Some(encoding_rs::WINDOWS_1252))
            .build(self.input.reader()?);
        let mut csv_rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .trim(csv::Trim::All)
            .from_reader(reader);
        let mut csv_records = csv_rdr.records();

        let acct_name: AccountName = deserialize_required_record(&mut csv_records)?
            .ok_or_else(|| anyhow!("bad file format: missing account name"))?;
        check_header("Account Name:", &acct_name.header)?;
        let balance: AccountQuantity = deserialize_required_record(&mut csv_records)?
            .ok_or_else(|| anyhow!("bad file format: missing account balance"))?;
        check_header("Account Balance:", &balance.header)?;
        let available: AccountQuantity = deserialize_required_record(&mut csv_records)?
            .ok_or_else(|| anyhow!("bad file format: missing available balance"))?;
        check_header("Available Balance:", &available.header)?;

        let fp_namespace = &self
            .commonopts
            .fp_ns
            .make_namespace(&acct_name.account_name);
        self.read_transactions(&mut csv_records, &fp_namespace, &acct_name.account_name)
    }
}

impl NationwideCsv {
    fn read_transactions<R: std::io::Read>(
        &self,
        csv_records: &mut csv::StringRecordsIter<R>,
        fp_prefix: &str,
        account_name: &str,
    ) -> Result<Vec<Transaction>> {
        let headers: Vec<String> = deserialize_required_record(csv_records)?
            .ok_or_else(|| anyhow!("bad file format: missing transaction headers"))?;
        if headers.len() != 6 {
            bail!("bad file format: expected 6 headers for transactions");
        }
        check_header("Date", &headers[0])?;
        check_header("Transaction type", &headers[1])?;
        check_header("Description", &headers[2])?;
        check_header("Paid out", &headers[3])?;
        check_header("Paid in", &headers[4])?;
        check_header("Balance", &headers[5])?;

        let mut transactions = Vec::new();

        let mut prev_date: Option<NaiveDate> = None;
        let mut date_counter: i32 = 0;

        for result in csv_records {
            let str_record = result?;
            let record: Record = str_record.deserialize(None)?;

            // Maintain the per-date counter. Include a sequence number to each
            // transaction in a given day for use in the fingerprint.
            if Some(record.date.0) != prev_date {
                prev_date = Some(record.date.0);
                date_counter = 0;
            } else {
                date_counter += 1;
            }

            let description = record.description.clone();
            let date = record.date.0;

            let (post1, post2) =
                self.form_postings(record, fp_prefix, account_name, date_counter)?;

            transactions.push(Transaction {
                date,
                description,
                comment: None,
                status: None,
                code: None,
                effective_date: None,
                postings: vec![post1, post2],
            });
        }

        Ok(transactions)
    }

    fn form_postings(
        &self,
        record: Record,
        fp_namespace: &str,
        account_name: &str,
        date_counter: i32,
    ) -> Result<(Posting, Posting)> {
        let self_amount: Amount = match (record.paid_in.clone(), record.paid_out.clone()) {
            // Paid in only.
            (Some(GbpValue(amt)), None) => amt,
            // Paid out only.
            (None, Some(GbpValue(amt))) => negate_amount(amt),
            // Paid in and out or neither - both are errors.
            _ => bail!("expected *either* paid in or paid out"),
        };

        let halves = self_and_peer_account_amount(self_amount, ASSETS_UNKNOWN.to_string());

        let mut self_comment = Comment::builder()
            .with_tag(tags::UNKNOWN_ACCOUNT)
            .with_value_tag(tags::ACCOUNT, account_name)
            .with_value_tag(tags::BANK, BANK_NAME)
            .with_value_tag(TRANSACTION_TYPE_TAG, record.type_.clone());
        let mut peer_comment = self_comment.clone();

        let fp_v1 = record.fingerprint_v1(fp_namespace, date_counter);
        self_comment = self_comment
            .with_tag(fp_v1.self_.tag())
            .with_tag(tags::IMPORT_SELF.to_string());
        peer_comment = peer_comment
            .with_tag(fp_v1.peer.tag())
            .with_tag(tags::IMPORT_PEER.to_string());

        if self.include_legacy_fingerprint {
            let fp_legacy = record.fingerprint_legacy(fp_namespace, date_counter, &halves);
            self_comment = self_comment.with_tag(fp_legacy.self_.legacy_tag());
            peer_comment = peer_comment.with_tag(fp_legacy.peer.legacy_tag());
        }

        Ok((
            Posting {
                account: halves.self_.account,
                amount: Some(halves.self_.amount),
                balance: Some(Balance::Amount(record.balance.0)),
                comment: self_comment.build().into_opt_comment(),
                status: None,
            },
            Posting {
                account: halves.peer.account,
                amount: Some(halves.peer.amount),
                balance: None,
                comment: peer_comment.build().into_opt_comment(),
                status: None,
            },
        ))
    }
}

mod de {
    use std::fmt;
    use std::str::FromStr;

    use anyhow::Result;
    use chrono::NaiveDate;
    use ledger_parser::{Amount, Commodity, CommodityPosition};
    use regex::Regex;
    use rust_decimal::Decimal;
    use serde::de::{self, Deserialize, DeserializeOwned, Deserializer};

    use crate::fingerprint::{Accumulator, FingerprintBuilder, Fingerprintable};
    use crate::importers::util::{
        self_and_peer_fingerprints, FingerprintHalves, TransactionHalves,
    };

    /// Contains the directly deserialized values from the six-column
    /// transaction format.
    #[derive(Debug, Deserialize)]
    pub struct Record {
        pub date: Date,
        pub type_: String,
        pub description: String,
        pub paid_out: Option<GbpValue>,
        pub paid_in: Option<GbpValue>,
        pub balance: GbpValue,
    }

    impl Record {
        /// An older and more flawed fingerprint, prior to including algorithm
        /// and version.
        pub fn fingerprint_legacy(
            &self,
            fp_namespace: &str,
            date_counter: i32,
            halves: &TransactionHalves,
        ) -> FingerprintHalves {
            let fpb_legacy = FingerprintBuilder::new("", 0, fp_namespace)
                .with(self.type_.as_str())
                .with(self.date.0)
                // Description should have been included in the legacy fingerprint, but a
                // bug left it blank.
                .with("")
                .with(&self.balance)
                .with(date_counter);

            let self_fp = fpb_legacy
                .clone()
                .with(halves.self_.account.as_str())
                .with(&halves.self_.amount);
            let peer_fp = fpb_legacy
                .with(halves.peer.account.as_str())
                .with(&halves.peer.amount);

            FingerprintHalves {
                self_: self_fp.build(),
                peer: peer_fp.build(),
            }
        }

        pub fn fingerprint_v1(&self, fp_namespace: &str, date_counter: i32) -> FingerprintHalves {
            self_and_peer_fingerprints(
                FingerprintBuilder::new("nwcsv6", 1, fp_namespace)
                    .with(self.type_.as_str())
                    .with(self.date.0)
                    .with(date_counter)
                    .with(self.description.as_str())
                    .with(self.paid_out.as_ref())
                    .with(self.paid_in.as_ref())
                    .with(&self.balance),
            )
        }
    }

    #[derive(Debug)]
    pub struct Date(pub NaiveDate);

    impl<'de> Deserialize<'de> for Date {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_str(DateVisitor)
        }
    }

    struct DateVisitor;
    impl<'de> de::Visitor<'de> for DateVisitor {
        type Value = Date;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a date string in \"DD Jan YYYY\" format")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            NaiveDate::parse_from_str(s, "%d %b %Y")
                .map(Date)
                .map_err(de::Error::custom)
        }
    }

    #[derive(Clone, Debug)]
    pub struct GbpValue(pub Amount);

    impl Fingerprintable for &GbpValue {
        fn fingerprint(self, acc: Accumulator) -> Accumulator {
            acc.with(&self.0)
        }
    }

    impl<'de> Deserialize<'de> for GbpValue {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_str(GbpValueVisitor)
        }
    }

    struct GbpValueVisitor;
    impl<'de> de::Visitor<'de> for GbpValueVisitor {
        type Value = GbpValue;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a monetary value string £NNN.NN format")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            lazy_static! {
                static ref RE: Regex = Regex::new(r"£(-?)(\d+)\.(\d+)").unwrap();
            }
            let captures = RE
                .captures(s)
                .ok_or_else(|| de::Error::custom("incorrect monetary format"))?;
            let is_negative: bool = captures.get(1).unwrap().as_str() == "-";
            let pounds: i64 = deserialize_captured_number(&captures, 2)?;
            let pence: i64 = deserialize_captured_number(&captures, 3)?;
            let mut quantity = Decimal::new(pounds * 100 + pence, 2);
            quantity.set_sign_negative(is_negative);
            Ok(GbpValue(Amount {
                commodity: Commodity {
                    name: "GBP".to_string(),
                    position: CommodityPosition::Left,
                },
                quantity,
            }))
        }
    }

    pub fn check_header(want: &'static str, got: &str) -> Result<()> {
        if want != got {
            bail!("bad header record, want {:?}, got {:?}", want, got);
        }
        Ok(())
    }

    fn deserialize_captured_number<T, E>(c: &regex::Captures, i: usize) -> Result<T, E>
    where
        T: FromStr,
        E: de::Error,
        <T as FromStr>::Err: fmt::Display,
    {
        c.get(i)
            .unwrap()
            .as_str()
            .parse()
            .map_err(de::Error::custom)
    }

    pub fn deserialize_required_record<T, R>(
        csv_records: &mut csv::StringRecordsIter<R>,
    ) -> Result<Option<T>>
    where
        T: DeserializeOwned,
        R: std::io::Read,
    {
        match csv_records.next() {
            Some(Ok(str_record)) => Ok(Some(str_record.deserialize(None)?)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::importers::nationwide::{CommonOpts, FpNamespace};
    use crate::importers::testutil::golden_test;

    use super::*;

    #[test]
    fn golden() {
        golden_test(
            &NationwideCsv {
                input: FileSpec::from_str("testdata/importers/nationwide_csv.csv").unwrap(),
                include_legacy_fingerprint: true,
                commonopts: CommonOpts {
                    fp_ns: FpNamespace::Generated,
                },
            },
            "nationwide_csv.golden.journal",
        );
    }
}
