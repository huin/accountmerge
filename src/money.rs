use std::convert::{TryFrom, TryInto};
use std::fmt;

#[derive(Debug, Fail)]
pub enum MoneyError {
    #[fail(display = "overflow in converting value {}", value)]
    Overflow { value: u32 },
    #[fail(display = "negative value {} in positive context", value)]
    Negative { value: i32 },
}

#[derive(Clone, Copy, Debug)]
pub struct UnsignedGbpValue {
    pub pence: u32,
}

impl UnsignedGbpValue {
    pub fn from_pence(pence: u32) -> Self {
        UnsignedGbpValue { pence }
    }

    pub fn parts(&self) -> (u32, u32) {
        (self.pence / 100, self.pence % 100)
    }
}

impl fmt::Display for UnsignedGbpValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let parts = self.parts();
        write!(f, "GBP {}.{:02}", parts.0, parts.1)
    }
}

impl TryFrom<GbpValue> for UnsignedGbpValue {
    type Error = MoneyError;

    fn try_from(value: GbpValue) -> Result<Self, Self::Error> {
        value
            .pence
            .try_into()
            .map(UnsignedGbpValue::from_pence)
            .map_err(|_| MoneyError::Negative { value: value.pence })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GbpValue {
    pub pence: i32,
}

impl GbpValue {
    pub fn from_parts(pounds: i32, pence: i32) -> Self {
        GbpValue::from_pence(pounds * 100 + pence)
    }

    pub fn from_pence(pence: i32) -> Self {
        GbpValue { pence }
    }

    pub fn parts(&self) -> (i32, i32) {
        (self.pence / 100, self.pence % 100)
    }
}

impl fmt::Display for GbpValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let parts = self.parts();
        write!(f, "GBP {}.{:02}", parts.0, parts.1.abs())
    }
}

impl std::ops::Neg for GbpValue {
    type Output = Self;

    fn neg(self) -> Self {
        GbpValue { pence: -self.pence }
    }
}

impl TryFrom<UnsignedGbpValue> for GbpValue {
    type Error = MoneyError;

    fn try_from(value: UnsignedGbpValue) -> Result<Self, Self::Error> {
        value
            .pence
            .try_into()
            .map(GbpValue::from_pence)
            .map_err(|_| MoneyError::Overflow { value: value.pence })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_gbp_value_display() {
        let tests: Vec<(u32, &'static str)> = vec![
            (0, "GBP 0.00"),
            (12, "GBP 0.12"),
            (123, "GBP 1.23"),
            (1234, "GBP 12.34"),
        ];
        for (pence, want) in tests {
            let v = UnsignedGbpValue { pence };
            let got = format!("{}", v);
            assert_eq!(want, got);
        }
    }

    #[test]
    fn gbp_value_display() {
        let tests: Vec<(i32, &'static str)> = vec![
            (0, "GBP 0.00"),
            (12, "GBP 0.12"),
            (123, "GBP 1.23"),
            (1234, "GBP 12.34"),
            (-1234, "GBP -12.34"),
        ];
        for (pence, want) in tests {
            let v = GbpValue { pence };
            let got = format!("{}", v);
            assert_eq!(want, got);
        }
    }
}
