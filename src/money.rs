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
        write!(f, "GbpValue({}.{})", parts.0, parts.1)
    }
}

impl fmt::Display for GbpValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let parts = self.parts();
        write!(f, "GBP {}.{}", parts.0, parts.1)
    }
}
