use std::convert::TryInto;

use byteorder::{BigEndian, ByteOrder};
use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use ledger_parser::Amount;
use sha1::{Digest, Sha1};

use crate::tags::FINGERPRINT_TAG_PREFIX;

pub fn make_prefix(value: &str) -> String {
    format!("{}{}-", FINGERPRINT_TAG_PREFIX, value)
}

pub trait Fingerprintable {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder;
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
        self.acc.build_with_prefix("")
    }

    pub fn build_with_prefix(self, prefix: &str) -> String {
        self.acc.build_with_prefix(prefix)
    }

    pub fn with<T>(self, v: T) -> Self
    where
        T: Fingerprintable,
    {
        v.fingerprint(self)
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

    fn add_bytes(&mut self, v: &[u8]) {
        self.hasher.input(v);
    }
}

impl Fingerprintable for &[u8] {
    fn fingerprint(self, mut fpb: FingerprintBuilder) -> FingerprintBuilder {
        fpb.acc.add_bytes(self);
        fpb
    }
}

impl Fingerprintable for i8 {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let buf: [u8; 1] = [self as u8];
        fpb.with(&buf[..])
    }
}

impl Fingerprintable for i16 {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let mut buf: [u8; 2] = Default::default();
        BigEndian::write_i16(&mut buf, self);
        fpb.with(&buf[..])
    }
}

impl Fingerprintable for i32 {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let mut buf: [u8; 4] = Default::default();
        BigEndian::write_i32(&mut buf, self);
        fpb.with(&buf[..])
    }
}

impl Fingerprintable for i64 {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let mut buf: [u8; 8] = Default::default();
        BigEndian::write_i64(&mut buf, self);
        fpb.with(&buf[..])
    }
}

impl Fingerprintable for u8 {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let buf: [u8; 1] = [self];
        fpb.with(&buf[..])
    }
}

impl Fingerprintable for u16 {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let mut buf: [u8; 2] = Default::default();
        BigEndian::write_u16(&mut buf, self);
        fpb.with(&buf[..])
    }
}

impl Fingerprintable for u32 {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let mut buf: [u8; 4] = Default::default();
        BigEndian::write_u32(&mut buf, self);
        fpb.with(&buf[..])
    }
}

impl Fingerprintable for u64 {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let mut buf: [u8; 8] = Default::default();
        BigEndian::write_u64(&mut buf, self);
        fpb.with(&buf[..])
    }
}

impl Fingerprintable for usize {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let v: u64 = self.try_into().expect("usize does not fit into u64");
        fpb.with(v)
    }
}

impl Fingerprintable for &Amount {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        let quantity: [u8; 16] = self.quantity.serialize();
        use ledger_parser::CommodityPosition::*;
        fpb.with(16usize + 1usize + self.commodity.name.len())
            .with(&quantity[..])
            .with(match self.commodity.position {
                Left => 1u8,
                Right => 2u8,
            })
            .with(self.commodity.name.as_str())
    }
}

impl<T> Fingerprintable for Option<T>
where
    T: Fingerprintable,
{
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        match self {
            Some(v) => fpb.with(1u8).with(v),
            None => fpb.with(0u8),
        }
    }
}

impl Fingerprintable for &str {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        fpb.with(self.len()).with(self.as_bytes())
    }
}

impl Fingerprintable for NaiveDate {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        fpb.with(3 * 4usize)
            .with(self.year())
            .with(self.month())
            .with(self.day())
    }
}

impl Fingerprintable for NaiveTime {
    fn fingerprint(self, fpb: FingerprintBuilder) -> FingerprintBuilder {
        fpb.with(4 * 4usize)
            .with(self.hour())
            .with(self.minute())
            .with(self.second())
            .with(self.nanosecond())
    }
}
