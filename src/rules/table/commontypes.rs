use std::fmt;

use serde::de;

#[derive(Debug)]
pub struct Regex(pub regex::Regex);

impl<'de> de::Deserialize<'de> for Regex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_str(RegexVisitor)
    }
}

struct RegexVisitor;

impl<'de> de::Visitor<'de> for RegexVisitor {
    type Value = Regex;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a string containing a regular expression")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        regex::Regex::new(v)
            .map(Regex)
            .map_err(|e| E::custom(format!("{}", e)))
    }
}
