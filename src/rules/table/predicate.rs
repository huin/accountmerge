use std::fmt;

#[cfg(test)]
use anyhow::Result;
use serde::de;
use serde::Deserialize;

use crate::rules::table::ctx::PostingContext;

#[derive(Debug, Deserialize)]
pub enum Predicate {
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Account(StringMatch),
    PostingFlagTag(StringMatch),
    PostingHasFlagTag(String),
    PostingHasValueTag(String),
    PostingValueTag(String, StringMatch),
    Not(Box<Predicate>),
    TransactionDescription(StringMatch),
    True,
}

impl Predicate {
    pub fn is_match(&self, ctx: &PostingContext) -> bool {
        use Predicate::*;
        match self {
            True => true,
            All(preds) => preds.iter().all(|p| p.is_match(ctx)),
            Any(preds) => preds.iter().any(|p| p.is_match(ctx)),
            Account(matcher) => matcher.matches_string(&ctx.post.raw.account),
            Not(pred) => !pred.is_match(ctx),
            PostingFlagTag(matcher) => ctx
                .post
                .comment
                .tags
                .iter()
                .any(|tag_name| matcher.matches_string(tag_name)),
            PostingHasFlagTag(tag_name) => ctx.post.comment.tags.contains(tag_name),
            PostingHasValueTag(tag_name) => ctx.post.comment.value_tags.contains_key(tag_name),
            PostingValueTag(tag_name, matcher) => ctx
                .post
                .comment
                .value_tags
                .get(tag_name)
                .map(|value| matcher.matches_string(value))
                .unwrap_or(false),
            TransactionDescription(matcher) => matcher.matches_string(&ctx.trn.raw.description),
        }
    }

    #[cfg(test)]
    pub fn from_str(s: &str) -> Result<Self> {
        ron::de::from_str(s).map_err(Into::into)
    }
}

#[derive(Debug)]
pub struct Regex(regex::Regex);

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

#[derive(Debug, Deserialize)]
pub enum StringMatch {
    AsLower(Box<StringMatch>),
    Contains(String),
    Eq(String),
    Matches(Regex),
}

impl StringMatch {
    fn matches_string(&self, s: &str) -> bool {
        use StringMatch::*;

        match self {
            AsLower(m) => m.matches_string(&s.to_lowercase()),
            Contains(want) => s.contains(want),
            Eq(want) => want == s,
            Matches(regex) => regex.0.is_match(s),
        }
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;
    use crate::testutil::parse_transaction_postings;

    const SIMPLE_POSTING: &str = r#"
        2000/01/01 Transaction description
            account:name  $10.00
            ; :flag-tag:
            ; value-tag: value-tag-value
            ; non-shouty-key: shouty-value
            ; shouty-key: SHOUTY-VALUE
    "#;

    #[test_case("Account(Contains(\"name\"))", SIMPLE_POSTING => true)]
    #[test_case("Account(Contains(\"other\"))", SIMPLE_POSTING => false)]
    #[test_case("Account(Eq(\"account:name\"))", SIMPLE_POSTING => true)]
    #[test_case("Account(Eq(\"account:other\"))", SIMPLE_POSTING => false)]
    #[test_case("Account(Matches(\"name\"))", SIMPLE_POSTING => true)]
    #[test_case("Account(Matches(\"^name\"))", SIMPLE_POSTING => false)]
    #[test_case("Not(True)", SIMPLE_POSTING => false)]
    #[test_case("PostingFlagTag(Matches(\"^flag-\"))", SIMPLE_POSTING => true)]
    #[test_case("PostingFlagTag(Matches(\"^no-such-flag\"))", SIMPLE_POSTING => false)]
    #[test_case("PostingHasFlagTag(\"flag-tag\")", SIMPLE_POSTING => true)]
    #[test_case("PostingHasFlagTag(\"other-flag-tag\")", SIMPLE_POSTING => false)]
    #[test_case("PostingHasValueTag(\"value-tag\")", SIMPLE_POSTING => true)]
    #[test_case("PostingHasValueTag(\"other-value-tag\")", SIMPLE_POSTING => false)]
    #[test_case("PostingValueTag(\"value-tag\", Eq(\"value-tag-value\"))", SIMPLE_POSTING => true)]
    #[test_case("PostingValueTag(\"value-tag\", Eq(\"other-value-tag-value\"))", SIMPLE_POSTING => false)]
    #[test_case("PostingValueTag(\"other-value-tag\", Eq(\"value-tag-value\"))", SIMPLE_POSTING => false)]
    #[test_case("PostingValueTag(\"non-shouty-key\", AsLower(Contains(\"shouty-value\")))", SIMPLE_POSTING => true)]
    #[test_case("PostingValueTag(\"shouty-key\", AsLower(Contains(\"shouty-value\")))", SIMPLE_POSTING => true)]
    #[test_case("PostingValueTag(\"shouty-key\", AsLower(Contains(\"SHOUTY-VALUE\")))", SIMPLE_POSTING => false)]
    #[test_case("TransactionDescription(Eq(\"Transaction description\"))", SIMPLE_POSTING => true)]
    #[test_case("TransactionDescription(Eq(\"non transaction description\"))", SIMPLE_POSTING => false)]
    #[test_case("True", SIMPLE_POSTING => true)]
    fn predicate(pred: &str, trn: &str) -> bool {
        let mut trn_post_set = parse_transaction_postings(trn);
        assert_eq!(1, trn_post_set.len());
        let trn_posts = &mut trn_post_set[0];
        assert_eq!(1, trn_posts.posts.len());
        let trn = &mut trn_posts.trn;
        let post = &mut trn_posts.posts[0];
        let ctx = PostingContext { trn, post };
        let predicate = Predicate::from_str(pred).expect("Predicate::from_str");
        predicate.is_match(&ctx)
    }
}
