use byteorder::{BigEndian, ByteOrder};
use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use ledger_parser::Amount;
use sha1::{Digest, Sha1};

use crate::tags;

/// Returns `true` if the tag is a fingerprint.
pub fn is_fingerprint(tag: &str) -> bool {
    tag.starts_with(tags::FINGERPRINT_PREFIX)
}

pub trait Fingerprintable {
    fn fingerprint(self, acc: Accumulator) -> Accumulator;
}

pub struct Fingerprint {
    algorithm_name: &'static str,
    algorithm_version: i64,
    user_namespace: String,
    value: String,
}

impl Fingerprint {
    pub fn legacy_tag(&self) -> String {
        format!(
            "{}{}-{}",
            tags::FINGERPRINT_PREFIX,
            self.user_namespace,
            self.value,
        )
    }

    pub fn tag(&self) -> String {
        format!(
            "{}{}.{}.{}-{}",
            tags::FINGERPRINT_PREFIX,
            self.algorithm_name,
            self.algorithm_version,
            self.user_namespace,
            self.value,
        )
    }
}

/// Builds a fingerprint based on length-prefixed values.
#[derive(Debug, Clone)]
pub struct FingerprintBuilder {
    acc: Accumulator,
    algorithm_name: &'static str,
    algorithm_version: i64,
    user_namespace: String,
}

impl FingerprintBuilder {
    pub fn new(algorithm_name: &'static str, algorithm_version: i64, user_namespace: &str) -> Self {
        Self {
            acc: Accumulator::new(),
            algorithm_name,
            algorithm_version,
            user_namespace: user_namespace.to_string(),
        }
    }

    pub fn build(self) -> Fingerprint {
        Fingerprint {
            algorithm_name: self.algorithm_name,
            algorithm_version: self.algorithm_version,
            user_namespace: self.user_namespace,
            value: self.acc.into_base64(),
        }
    }

    pub fn with<T>(self, v: T) -> Self
    where
        T: Fingerprintable,
    {
        Self {
            acc: v.fingerprint(self.acc),
            algorithm_name: self.algorithm_name,
            algorithm_version: self.algorithm_version,
            user_namespace: self.user_namespace,
        }
    }
}

/// Builds parts of a fingerprint based on raw values.
///
/// This does *not* write length prefixes, unlike `FingerprintBuilder`, but is
/// used *by* `FingerprintBuilder`.
#[derive(Debug, Clone)]
pub struct Accumulator {
    hasher: Sha1,
}

impl Accumulator {
    pub fn new() -> Self {
        Self {
            hasher: Sha1::new(),
        }
    }

    pub fn into_base64(self) -> String {
        base64::display::Base64Display::new(
            &self.hasher.finalize(),
            &base64::engine::general_purpose::STANDARD_NO_PAD,
        )
        .to_string()
    }

    fn add_bytes(&mut self, v: &[u8]) {
        self.hasher.update(v);
    }

    pub fn with<T>(self, v: T) -> Self
    where
        T: Fingerprintable,
    {
        v.fingerprint(self)
    }
}

impl Fingerprintable for &[u8] {
    fn fingerprint(self, mut acc: Accumulator) -> Accumulator {
        acc.add_bytes(self);
        acc
    }
}

impl Fingerprintable for i8 {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let buf: [u8; 1] = [self as u8];
        acc.with(&buf[..])
    }
}

impl Fingerprintable for i16 {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let mut buf: [u8; 2] = Default::default();
        BigEndian::write_i16(&mut buf, self);
        acc.with(&buf[..])
    }
}

impl Fingerprintable for i32 {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let mut buf: [u8; 4] = Default::default();
        BigEndian::write_i32(&mut buf, self);
        acc.with(&buf[..])
    }
}

impl Fingerprintable for i64 {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let mut buf: [u8; 8] = Default::default();
        BigEndian::write_i64(&mut buf, self);
        acc.with(&buf[..])
    }
}

impl Fingerprintable for u8 {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let buf: [u8; 1] = [self];
        acc.with(&buf[..])
    }
}

impl Fingerprintable for u16 {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let mut buf: [u8; 2] = Default::default();
        BigEndian::write_u16(&mut buf, self);
        acc.with(&buf[..])
    }
}

impl Fingerprintable for u32 {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let mut buf: [u8; 4] = Default::default();
        BigEndian::write_u32(&mut buf, self);
        acc.with(&buf[..])
    }
}

impl Fingerprintable for u64 {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let mut buf: [u8; 8] = Default::default();
        BigEndian::write_u64(&mut buf, self);
        acc.with(&buf[..])
    }
}

impl Fingerprintable for usize {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let v: u64 = self.try_into().expect("usize does not fit into u64");
        acc.with(v)
    }
}

impl Fingerprintable for &Amount {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        let quantity: [u8; 16] = self.quantity.serialize();
        use ledger_parser::CommodityPosition::*;
        acc.with(16usize + 1usize + self.commodity.name.len())
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
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        match self {
            Some(v) => acc.with(1u8).with(v),
            None => acc.with(0u8),
        }
    }
}

impl Fingerprintable for &str {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        acc.with(self.len()).with(self.as_bytes())
    }
}

impl Fingerprintable for NaiveDate {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        acc.with(3 * 4usize)
            .with(self.year())
            .with(self.month())
            .with(self.day())
    }
}

impl Fingerprintable for NaiveTime {
    fn fingerprint(self, acc: Accumulator) -> Accumulator {
        acc.with(4 * 4usize)
            .with(self.hour())
            .with(self.minute())
            .with(self.second())
            .with(self.nanosecond())
    }
}
