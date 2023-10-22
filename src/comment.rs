use std::collections::{HashMap, HashSet};

use lazy_static::lazy_static;
use regex::Regex;

/// Maximum length of a tag before it gets put onto a line on its own.
const MAX_INLINE_TAG_LEN: usize = 12;

/// Parsed contents of a Ledger comment, suitable for manipulation before being
/// (re)output.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Comment {
    /// Plain text lines in the comment.
    pub lines: Vec<String>,
    /// Tags that are present or not, e.g: `":TAG:"`.
    pub tags: HashSet<String>,
    /// Tags that have a string value, e.g: `"TAG: value"`.
    pub value_tags: HashMap<String, String>,
}

impl Comment {
    /// Creates an empty `Comment`.
    pub fn new() -> Self {
        Self {
            lines: Default::default(),
            tags: Default::default(),
            value_tags: Default::default(),
        }
    }

    /// Starts declarative creation of a `Comment`.
    pub fn builder() -> CommentBuilder {
        CommentBuilder::new()
    }

    pub fn from_opt_string(comment: &Option<String>) -> Self {
        Self::from_opt_comment(comment.as_ref().map(String::as_str))
    }

    /// Parses the given string into a `Comment`.
    pub fn from_opt_comment(comment: Option<&str>) -> Self {
        lazy_static! {
            static ref VALUE_TAG_RX: Regex = Regex::new(r"^[ ]*([^: ]+):(?:[ ]+(.+))?$").unwrap();
        }
        lazy_static! {
            static ref FLAG_TAG_RX: Regex = Regex::new(r":((?:[^: ]+:)+)").unwrap();
        }

        let mut result = Comment::new();

        let comment: &str = match comment {
            Some(s) => s,
            None => return result,
        };

        for line in comment.split('\n') {
            // Value tags comprise an entire comment line.
            if let Some(kv_parts) = VALUE_TAG_RX.captures(line) {
                let key = kv_parts
                    .get(1)
                    .expect("should always have group 1")
                    .as_str();
                let value = kv_parts.get(2).map(|c| c.as_str()).unwrap_or("");
                result.value_tags.insert(key.to_string(), value.to_string());
            } else {
                // Flag tag groups can be mixed into a line with comment text.
                let mut leading_start: usize = 0;
                for flag_group in FLAG_TAG_RX.captures_iter(line) {
                    // Found flags (maybe with text before them).

                    let all = flag_group.get(0).expect("should always have group 0");
                    let flags = flag_group.get(1).expect("should always have group 1");
                    let leading_end = all.start();
                    if leading_start < leading_end {
                        // Found text prior to flags.
                        let text = line[leading_start..leading_end].trim();
                        if !text.is_empty() {
                            result.lines.push(text.to_string());
                        }
                    }
                    leading_start = all.end();

                    // Flags.
                    for flag in flags.as_str().trim_end_matches(':').split(':') {
                        result.tags.insert(flag.to_string());
                    }
                }
                if leading_start < line.len() {
                    let text = line[leading_start..].trim();
                    if !text.is_empty() {
                        result.lines.push(text.to_string());
                    }
                }
            }
        }
        result
    }

    /// Formats this `Comment` into a string.
    pub fn into_opt_comment(self) -> Option<String> {
        let mut out_lines = Vec::<String>::new();

        if !self.tags.is_empty() {
            let (mut short_tags, mut long_tags): (Vec<String>, Vec<String>) = self
                .tags
                .into_iter()
                .partition(|tag| tag.len() <= MAX_INLINE_TAG_LEN);

            if !short_tags.is_empty() {
                short_tags.sort();
                out_lines.push(format!(":{}:", short_tags.join(":")));
            }

            // Put any long tags onto a line of their own.
            long_tags.sort();
            out_lines.extend(long_tags.into_iter().map(|tag| format!(":{}:", tag)));
        }
        for (i, line) in self.lines.into_iter().enumerate() {
            if i == 0 && !out_lines.is_empty() {
                // Compress test comment onto first line with tags if possible
                // to reduce number of output lines.
                out_lines[0].push(' ');
                out_lines[0].push_str(line.trim());
            } else {
                out_lines.push(trim_string(line));
            }
        }

        let mut sorted_entries: Vec<(String, String)> = self.value_tags.into_iter().collect();
        sorted_entries.sort();
        for (k, v) in sorted_entries.into_iter() {
            out_lines.push(format!("{}: {}", k.trim(), v.trim()));
        }

        if !out_lines.is_empty() {
            Some(out_lines.join("\n"))
        } else {
            None
        }
    }

    /// Merges tags and lines from `other` into `self`. Values from
    /// `other.value_tags` will overwrite values in `self.value_tags` where
    /// they share a key. It avoids adding duplicate lines from `other.lines`
    /// if an exact match already exists in `self.lines`.
    pub fn merge_from(&mut self, other: Self) {
        for other_line in other.lines.into_iter() {
            if !self.lines.iter().any(|self_line| self_line == &other_line) {
                self.lines.push(other_line);
            }
        }
        self.tags.extend(other.tags);
        self.value_tags.extend(other.value_tags);
    }
}

fn trim_string(s: String) -> String {
    if s.trim().len() == s.len() {
        s
    } else {
        s.trim().to_string()
    }
}

/// Helper to declaratively define a `Comment`.
#[derive(Clone)]
pub struct CommentBuilder {
    comment: Comment,
}

impl CommentBuilder {
    fn new() -> Self {
        CommentBuilder {
            comment: Comment::new(),
        }
    }

    /// Builds the final `Comment`.
    pub fn build(self) -> Comment {
        self.comment
    }

    #[cfg(test)] // Currently only used in tests.
    pub fn with_line<S: Into<String>>(mut self, line: S) -> Self {
        self.comment.lines.push(line.into());
        self
    }

    pub fn with_option_tag<K: Into<String>>(mut self, k: Option<K>) -> Self {
        if let Some(k) = k {
            self.comment.tags.insert(k.into());
        }
        self
    }

    pub fn with_tag<K: Into<String>>(mut self, k: K) -> Self {
        self.comment.tags.insert(k.into());
        self
    }

    pub fn with_option_value_tag<K: Into<String>, V: Into<String>>(
        mut self,
        k: K,
        v: Option<V>,
    ) -> Self {
        if let Some(v) = v {
            self.comment.value_tags.insert(k.into(), v.into());
        }
        self
    }

    pub fn with_value_tag<K: Into<String>, V: Into<String>>(mut self, k: K, v: V) -> Self {
        self.comment.value_tags.insert(k.into(), v.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use test_case::test_case;

    #[test_case(
        ""
        => Comment::new();
        "empty_comment"
    )]
    #[test_case(
        "comment text"
        => CommentBuilder::new().with_line("comment text").build();
        "just_text"
    )]
    #[test_case(
        "start text\nkey: value\nend text"
        => CommentBuilder::new()
            .with_line("start text")
            .with_value_tag("key", "value")
            .with_line("end text")
            .build();
        "text_kv_text"
    )]
    #[test_case(
        "start text :TAG1:TAG2: end text\n"
        => CommentBuilder::new()
            .with_line("start text")
            .with_tag("TAG1")
            .with_tag("TAG2")
            .with_line("end text")
            .build();
        "text_2_inline_tags_text"
    )]
    #[test_case(
        "start text :TAG1:TAG2: end : text : with : colons\n"
        => CommentBuilder::new()
            .with_line("start text")
            .with_tag("TAG1")
            .with_tag("TAG2")
            .with_line("end : text : with : colons")
            .build();
        "text_2_inline_tags_text_with_colons"
    )]
    #[test_case(
        "comment\n:flag: ignored-key: value\nkey: value"
        => CommentBuilder::new()
            .with_line("comment")
            .with_tag("flag")
            .with_line("ignored-key: value") // Badly formed value tag becomes text.
            .with_value_tag("key", "value")
            .build();
        "bad_key_value_becomes_text"
    )]
    #[test_case(
        "comment\nkey-without-value:"
        => CommentBuilder::new()
            .with_line("comment")
            .with_value_tag("key-without-value", "")
            .build();
        "key_without_value"
    )]
    fn test_parse_comment(text: &str) -> Comment {
        Comment::from_opt_comment(Some(text))
    }

    #[test_case(
        Comment::new()
        => None;
        "empty_comment_to_none"
    )]
    #[test_case(
        CommentBuilder::new()
            .with_line("first line")
            .with_line("second line")
            .build()
        => Some("first line\nsecond line".to_string());
        "two_lines_of_text"
    )]
    #[test_case(
        CommentBuilder::new()
            .with_line("first line")
            .with_line("second line")
            .with_value_tag("name", "value")
            .build()
        => Some("first line\nsecond line\nname: value".to_string());
        "two_lines_of_text_and_key_value"
    )]
    #[test_case(
        CommentBuilder::new()
            .with_line("text")
            .with_tag("tag1")
            .with_tag("tag2")
            .with_line("more text")
            .with_tag("tag3")
            .with_tag("tag4")
            .build()
        => Some(":tag1:tag2:tag3:tag4: text\nmore text".to_string());
        "four_inline_tags_followed_by_text"
    )]
    #[test_case(
        CommentBuilder::new()
            .with_line("text")
            .with_tag("tag1")
            .with_tag("tag2")
            .with_value_tag("name1", "value1")
            .with_line("more text")
            .with_tag("tag3")
            .with_value_tag("name2", "value2")
            .build()
        => Some(":tag1:tag2:tag3: text\nmore text\nname1: value1\nname2: value2".to_string());
        "newlines_injected_when_needed"
    )]
    #[test_case(
        CommentBuilder::new()
            .with_tag("a_tag")
            .with_tag("z_tag")
            .with_value_tag("name1", "value1")
            .with_tag("really_long_tag_name")
            .build()
        => Some(":a_tag:z_tag:\n:really_long_tag_name:\nname1: value1".to_string());
        "long_tags_go_on_own_line"
    )]
    fn test_format_comment(comment: Comment) -> Option<String> {
        comment.into_opt_comment()
    }

    #[test]
    fn test_merge_comment() {
        let mut orig = CommentBuilder::new()
            .with_line("orig text")
            .with_value_tag("orig_key1", "orig_value1")
            .with_value_tag("orig_key2", "orig_value2")
            .with_tag("orig_tag")
            .build();
        orig.merge_from(
            CommentBuilder::new()
                .with_line("new text")
                .with_value_tag("new_key1", "new_value1")
                .with_value_tag("orig_key2", "new_value2")
                .with_tag("new_tag")
                .build(),
        );
        assert_eq!(
            CommentBuilder::new()
                .with_line("orig text")
                .with_line("new text")
                .with_value_tag("new_key1", "new_value1")
                .with_value_tag("orig_key1", "orig_value1")
                .with_value_tag("orig_key2", "new_value2")
                .with_tag("new_tag")
                .with_tag("orig_tag")
                .build(),
            orig,
        );
    }
}
