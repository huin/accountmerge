use std::fmt::{self, Formatter};

use regex::Regex;

/// Tag that has a string value, e.g: `"TAG: value"`.
#[derive(Debug, Eq, PartialEq)]
pub struct ValueTag {
    name: String,
    value: String,
}

impl ValueTag {
    pub fn new<S1: Into<String>, S2: Into<String>>(name: S1, value: S2) -> Self {
        ValueTag {
            name: name.into(),
            value: value.into(),
        }
    }
}

impl fmt::Display for ValueTag {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        write!(f, "{}: {}", self.name.trim(), self.value.trim())
    }
}

/// Tag that is present or not, e.g: `":TAG:"`.
#[derive(Debug, Eq, PartialEq)]
pub struct FlagTag(String);

impl FlagTag {
    #[cfg(test)]
    fn new<S: Into<String>>(name: S) -> Self {
        FlagTag(name.into())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum CommentLine {
    /// Line containing [`FlagTag`]s and/or text.
    Line(Vec<CommentLinePart>),
    /// Line containing only a [`ValueTag`].
    ValueTag(ValueTag),
}

impl CommentLine {
    pub fn value_tag<S1: Into<String>, S2: Into<String>>(name: S1, value: S2) -> Self {
        CommentLine::ValueTag(ValueTag::new(name, value))
    }

    fn has_flag_tag(&self, find_name: &str) -> bool {
        match self {
            CommentLine::Line(parts) => parts.iter().any(|part| part.has_flag_tag(find_name)),
            _ => false,
        }
    }

    fn has_value_tag(&self, find_name: &str) -> bool {
        match self {
            CommentLine::ValueTag(tag) => tag.name == find_name,
            _ => false,
        }
    }

    fn remove_flag_tag(&mut self, find_name: &str) {
        match self {
            CommentLine::Line(parts) => {
                for part in parts {
                    part.remove_flag_tag(find_name);
                }
            }
            _ => {}
        }
    }
}

impl fmt::Display for CommentLine {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        match self {
            CommentLine::ValueTag(vt) => write!(f, "{}", vt),
            CommentLine::Line(parts) => {
                for part in parts {
                    match part {
                        CommentLinePart::Text(text) => f.write_str(&text)?,
                        CommentLinePart::FlagTags(tags) => {
                            if tags.len() > 0 {
                                for tag in tags {
                                    f.write_str(":")?;
                                    f.write_str(&tag.0.trim())?;
                                }
                                f.write_str(":")?;
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum CommentLinePart {
    FlagTags(Vec<FlagTag>),
    Text(String),
}

impl CommentLinePart {
    fn has_flag_tag(&self, find_name: &str) -> bool {
        match self {
            CommentLinePart::FlagTags(tags) => tags.iter().any(|tag| tag.0 == find_name),
            _ => false,
        }
    }

    fn remove_flag_tag(&mut self, find_name: &str) {
        if !self.has_flag_tag(find_name) {
            return;
        }
        match self {
            CommentLinePart::FlagTags(tags) => tags.retain(|tag| tag.0 != find_name),
            _ => {}
        }
    }
}

fn format_comment(lines: &Vec<CommentLine>) -> Option<String> {
    let out_lines: Vec<String> = lines
        .iter()
        .map(|line| format!("{}", line).trim().to_string())
        .collect();
    let comment = out_lines.join("\n");
    if comment.trim() == "" {
        None
    } else {
        Some(comment)
    }
}

fn parse_comment(s: &str) -> Vec<CommentLine> {
    lazy_static! {
        static ref VALUE_TAG_RX: Regex = Regex::new(r"^[ ]*([^: ]+):(?:[ ]+(.+))?$").unwrap();
    }
    lazy_static! {
        static ref FLAG_TAG_RX: Regex = Regex::new(r":((?:[^: ]+:)+)").unwrap();
    }
    if s == "" {
        return Vec::new();
    }
    let mut parts = Vec::new();
    for line in s.split('\n') {
        // Value tags comprise an entire comment line.
        if let Some(kv_parts) = VALUE_TAG_RX.captures(line) {
            let key = kv_parts
                .get(1)
                .expect("should always have group 1")
                .as_str();
            let value = kv_parts.get(2).map(|c| c.as_str()).unwrap_or("");
            parts.push(CommentLine::ValueTag(ValueTag::new(
                key.to_string(),
                value.to_string(),
            )));
        } else {
            // Flag tag groups can be mixed into a line with comment text.
            let mut leading_start: usize = 0;
            let mut parsed_line = Vec::<CommentLinePart>::new();
            for flag_group in FLAG_TAG_RX.captures_iter(line) {
                // Found flags (maybe with text before them).

                let all = flag_group.get(0).expect("should always have group 0");
                let flags = flag_group.get(1).expect("should always have group 1");
                let leading_end = all.start();
                if leading_start < leading_end {
                    // Found text prior to flags.
                    parsed_line.push(CommentLinePart::Text(
                        line[leading_start..leading_end].to_string(),
                    ));
                }
                leading_start = all.end();

                // Flags.
                let mut parsed_flags = Vec::<FlagTag>::new();
                println!("{:?} {:?}", flags, flags.as_str());
                for flag in flags.as_str().trim_end_matches(':').split(':') {
                    parsed_flags.push(FlagTag(flag.to_string()));
                }
                parsed_line.push(CommentLinePart::FlagTags(parsed_flags));
            }
            if leading_start < line.len() {
                parsed_line.push(CommentLinePart::Text(line[leading_start..].to_string()));
            }
            parts.push(CommentLine::Line(parsed_line));
        }
    }
    parts
}

pub struct CommentLines {
    lines: Vec<CommentLine>,
}

impl CommentLines {
    pub fn new() -> Self {
        Self { lines: vec![] }
    }

    pub fn push_line(&mut self, comment: CommentLine) {
        self.lines.push(comment);
    }

    pub fn from_opt_comment(comment: &Option<String>) -> Self {
        CommentLines {
            lines: comment
                .as_ref()
                .map(|c| parse_comment(&c))
                .unwrap_or_else(|| Vec::new()),
        }
    }

    pub fn to_opt_comment(&self) -> Option<String> {
        if self.lines.len() > 0 {
            format_comment(&self.lines)
        } else {
            None
        }
    }

    pub fn get_value_tag(&self, find_name: &str) -> Option<&str> {
        for line in &self.lines {
            match line {
                CommentLine::ValueTag(tag) if tag.name == find_name => return Some(&tag.value),
                _ => {}
            }
        }
        None
    }

    pub fn has_flag_tag(&self, find_name: &str) -> bool {
        for line in &self.lines {
            if line.has_flag_tag(find_name) {
                return true;
            }
        }
        false
    }

    pub fn remove_flag_tag(&mut self, find_name: &str) {
        for line in &mut self.lines {
            line.remove_flag_tag(find_name);
        }
    }

    pub fn has_value_tag(&self, find_name: &str) -> bool {
        self.lines.iter().any(|line| line.has_value_tag(find_name))
    }

    pub fn remove_value_tag(&mut self, find_name: &str) {
        self.lines.retain(|line| match line {
            CommentLine::ValueTag(tag) => tag.name != find_name,
            _ => true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CommentLineBuilder(Vec<CommentLinePart>);
    impl CommentLineBuilder {
        fn new() -> Self {
            Self(Vec::new())
        }
        fn flag_tags(mut self, names: &[&str]) -> Self {
            let tags: Vec<FlagTag> = names.iter().map(|n| FlagTag::new(*n)).collect();
            self.0.push(CommentLinePart::FlagTags(tags));
            self
        }
        fn text(mut self, text: &'static str) -> Self {
            self.0.push(CommentLinePart::Text(text.to_string()));
            self
        }
        fn build(self) -> CommentLine {
            CommentLine::Line(self.0)
        }
    }

    #[test]
    fn test_parse_comment() {
        let empty = Vec::<CommentLine>::new();
        assert_eq!(empty, parse_comment(""));
        assert_eq!(
            vec![CommentLineBuilder::new().text("comment text").build()],
            parse_comment("comment text")
        );
        assert_eq!(
            vec![
                CommentLineBuilder::new().text("start text").build(),
                CommentLine::value_tag("key", "value"),
                CommentLineBuilder::new().text("end text").build(),
            ],
            parse_comment("start text\nkey: value\nend text"),
        );
        assert_eq!(
            vec![
                CommentLineBuilder::new()
                    .text("start text ")
                    .flag_tags(&["TAG1", "TAG2"])
                    .text(" end text")
                    .build(),
                CommentLineBuilder::new().build(),
            ],
            parse_comment("start text :TAG1:TAG2: end text\n"),
        );
        assert_eq!(
            vec![
                CommentLineBuilder::new()
                    .text("start text ")
                    .flag_tags(&["TAG1", "TAG2"])
                    .text(" end : text : with : colons")
                    .build(),
                CommentLineBuilder::new().build(),
            ],
            parse_comment("start text :TAG1:TAG2: end : text : with : colons\n"),
        );
        assert_eq!(
            vec![
                CommentLineBuilder::new().text("comment").build(),
                CommentLineBuilder::new()
                    .flag_tags(&["flag"])
                    .text(" ignored-key: value")
                    .build(),
                CommentLine::value_tag("key", "value"),
            ],
            parse_comment("comment\n:flag: ignored-key: value\nkey: value"),
        );
        assert_eq!(
            vec![
                CommentLineBuilder::new().text("comment").build(),
                CommentLine::value_tag("key-without-value", ""),
            ],
            parse_comment("comment\nkey-without-value:"),
        );
    }

    #[test]
    fn test_format_comment() {
        assert_eq!(None, format_comment(&vec![]));
        assert_eq!(
            Some("first line\nsecond line".to_string()),
            format_comment(&vec![
                CommentLineBuilder::new().text("first line").build(),
                CommentLineBuilder::new().text("second line").build(),
            ]),
        );
        assert_eq!(
            Some("first line\nsecond line\nname: value".to_string()),
            format_comment(&vec![
                CommentLineBuilder::new().text("first line").build(),
                CommentLineBuilder::new().text("second line").build(),
                CommentLine::value_tag("name", "value"),
            ]),
        );
        assert_eq!(
            Some("text :tag1:tag2: more text :tag3:\n:tag4:".to_string()),
            format_comment(&vec![
                CommentLineBuilder::new()
                    .text("text ")
                    .flag_tags(&["tag1", "tag2"])
                    .text(" more text ")
                    .flag_tags(&["tag3"])
                    .build(),
                CommentLineBuilder::new().flag_tags(&["tag4"]).build(),
            ]),
        );
        // Are newlines injected when needed, even if not specified?
        assert_eq!(
            Some("text :tag1:tag2:\nname1: value1\nmore text :tag3:\nname2: value2".to_string()),
            format_comment(&vec![
                CommentLineBuilder::new()
                    .text("text ")
                    .flag_tags(&["tag1", "tag2"])
                    .build(),
                CommentLine::value_tag("name1", "value1"),
                CommentLineBuilder::new()
                    .text("more text ")
                    .flag_tags(&["tag3"])
                    .build(),
                CommentLine::value_tag("name2", "value2"),
            ]),
        );
    }
}
