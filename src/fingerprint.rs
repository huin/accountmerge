use std::convert::TryInto;

use byteorder::{BigEndian, ByteOrder};
use chrono::{Datelike, NaiveDate};
use ledger_parser::Amount;
use sha1::{Digest, Sha1};

pub trait Fingerprintable {
    fn fingerprint(&self, fpb: FingerprintBuilder) -> FingerprintBuilder;
}

/// Builds a fingerprint based on length-prefixed values.
#[derive(Debug, Clone)]
pub struct FingerprintBuilder {
    acc: Accumulator,
}

impl FingerprintBuilder {
    pub fn new() -> Self {
        Self {
            acc: Accumulator::new(),
        }
    }

    pub fn build(self) -> String {
        self.build_with_prefix("")
    }

    pub fn build_with_prefix(self, prefix: &str) -> String {
        self.acc.build_with_prefix(prefix)
    }

    pub fn with_amount(self, v: &Amount) -> Self {
        let quantity: [u8; 16] = Default::default();
        use ledger_parser::CommodityPosition::*;
        self.acc
            .with_usize(16 + 1 + v.commodity.name.len())
            .with_bytes(&quantity)
            .with_u8(match v.commodity.position {
                Left => 1,
                Right => 2,
            })
            .with_str(&v.commodity.name)
            .as_fingerprint_builder()
    }

    pub fn with_fingerprintable<T>(self, v: &T) -> Self
    where
        T: Fingerprintable,
    {
        v.fingerprint(self)
    }

    pub fn with_naive_date(self, v: &NaiveDate) -> Self {
        self.acc
            .with_usize(3 * 4)
            .with_i32(v.year())
            .with_u32(v.month() as u32)
            .with_u32(v.day() as u32)
            .as_fingerprint_builder()
    }

    pub fn with_str(self, v: &str) -> Self {
        self.acc
            .with_usize(v.len())
            .with_str(v)
            .as_fingerprint_builder()
    }
}

/// Builds parts of a fingerprint based on raw values.
///
/// This does *not* write length prefixes, unlike `FingerprintBuilder`, but is
/// used *by* `FingerprintBuilder`.
#[derive(Debug, Clone)]
struct Accumulator {
    hasher: Sha1,
}

impl Accumulator {
    fn new() -> Self {
        Self {
            hasher: Sha1::new(),
        }
    }

    fn build_with_prefix(self, prefix: &str) -> String {
        use base64::display::Base64Display;
        format!(
            "{}{}",
            prefix,
            Base64Display::with_config(&self.hasher.result(), base64::STANDARD_NO_PAD)
        )
    }

    fn as_fingerprint_builder(self) -> FingerprintBuilder {
        FingerprintBuilder { acc: self }
    }

    fn with_bytes(mut self, v: &[u8]) -> Self {
        self.hasher.input(v);
        self
    }

    fn with_i32(self, v: i32) -> Self {
        let mut buf: [u8; 4] = Default::default();
        BigEndian::write_i32(&mut buf, v);
        self.with_bytes(&buf)
    }

    fn with_str(self, v: &str) -> Self {
        self.with_bytes(v.as_bytes())
    }

    fn with_u32(self, v: u32) -> Self {
        let mut buf: [u8; 4] = Default::default();
        BigEndian::write_u32(&mut buf, v);
        self.with_bytes(&buf)
    }

    fn with_u8(self, v: u8) -> Self {
        let buf: [u8; 1] = [v];
        self.with_bytes(&buf)
    }

    fn with_u64(self, v: u64) -> Self {
        let mut buf: [u8; 8] = Default::default();
        BigEndian::write_u64(&mut buf, v);
        self.with_bytes(&buf)
    }

    fn with_usize(self, v: usize) -> Self {
        self.with_u64(v.try_into().expect("usize does not fit into u64"))
    }
}
