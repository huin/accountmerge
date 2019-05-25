use std::fmt;

pub struct GbpValue {
    pub pence: i32,
}

impl GbpValue {
    pub fn parts(&self) -> (i32, i32) {
        (self.pence / 100, self.pence % 100)
    }
}

impl fmt::Debug for GbpValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let parts = self.parts();
        write!(f, "GbpValue({}.{:02})", parts.0, parts.1.abs())
    }
}

impl fmt::Display for GbpValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let parts = self.parts();
        write!(f, "GBP {}.{:02}", parts.0, parts.1.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
