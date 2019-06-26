use regex::Regex;

#[derive(Debug, Eq, PartialEq)]
pub enum CommentPart {
    /// Tag that is present or not, e.g: `":TAG:"`.
    FlagTag(String),
    /// Tag that has a string value, e.g: `"TAG: value"`.
    ValueTag(String, String),
    /// Non-tag comment content.
    Text(String),
    /// Newline in comment.
    NewLine,
}

impl CommentPart {
    #[cfg(test)]
    pub fn flag_tag<S: Into<String>>(name: S) -> Self {
        CommentPart::FlagTag(name.into())
    }

    #[cfg(test)]
    pub fn text<S: Into<String>>(text: S) -> Self {
        CommentPart::Text(text.into())
    }

    pub fn value_tag<S1: Into<String>, S2: Into<String>>(name: S1, value: S2) -> Self {
        CommentPart::ValueTag(name.into(), value.into())
    }
}

fn format_comment(parts: &Vec<CommentPart>) -> String {
    use CommentPart::*;
    let mut out_parts: Vec<String> = Vec::new();
    let mut prev_part: Option<&CommentPart> = None;
    for cur_part in parts {
        match cur_part {
            FlagTag(name) => {
                match prev_part {
                    Some(FlagTag(_)) => {}
                    _ => out_parts.push(":".to_string()),
                }
                out_parts.push(format!("{}:", name));
            }

            ValueTag(name, value) => {
                match prev_part {
                    None => {}
                    Some(NewLine) => {}
                    _ => out_parts.push("\n".to_string()),
                }
                out_parts.push(format!("{}: {}", name, value));
            }

            Text(text) => {
                match prev_part {
                    Some(ValueTag(_, _)) => out_parts.push("\n".to_string()),
                    _ => {}
                }
                let trimmed = match prev_part {
                    Some(ValueTag(_, _)) => text.trim_start(),
                    Some(NewLine) => text.trim_start(),
                    None => text.trim_start(),
                    _ => text,
                };
                out_parts.push(trimmed.to_string());
            }

            NewLine => {
                if prev_part.is_some() {
                    out_parts.push("\n".to_string());
                }
            }
        }
        prev_part = Some(cur_part);
    }
    out_parts.join("")
}

fn parse_comment(s: &str) -> Vec<CommentPart> {
    lazy_static! {
        static ref VALUE_TAG_RX: Regex = Regex::new(r"^[ ]*([^: ]+):(?:[ ]+(.+))?$").unwrap();
    }
    lazy_static! {
        static ref FLAG_TAG_RX: Regex = Regex::new(r":((?:[^: ]+:)+)").unwrap();
    }
    let mut parts = Vec::new();
    for (i, line) in s.split('\n').enumerate() {
        if i != 0 && line != "" {
            parts.push(CommentPart::NewLine);
        }
        // Value tags comprise an entire comment line.
        if let Some(kv_parts) = VALUE_TAG_RX.captures(line) {
            let key = kv_parts
                .get(1)
                .expect("should always have group 1")
                .as_str();
            let value = kv_parts.get(2).map(|c| c.as_str()).unwrap_or("");
            parts.push(CommentPart::ValueTag(key.to_string(), value.to_string()));
        } else {
            // Flag tag groups can be mixed into a line with comment text.
            let mut leading_start: usize = 0;
            for flag_group in FLAG_TAG_RX.captures_iter(line) {
                let all = flag_group.get(0).expect("should always have group 0");
                let flags = flag_group.get(1).expect("should always have group 1");
                let leading_end = all.start();
                if leading_start < leading_end {
                    parts.push(CommentPart::Text(
                        line[leading_start..leading_end].to_string(),
                    ));
                }
                leading_start = all.end();

                println!("{:?} {:?}", flags, flags.as_str());
                for flag in flags.as_str().trim_end_matches(':').split(':') {
                    parts.push(CommentPart::FlagTag(flag.to_string()));
                }
            }
            if leading_start < line.len() {
                parts.push(CommentPart::Text(line[leading_start..].to_string()));
            }
        }
    }
    parts
}

pub struct CommentManipulator {
    parts: Vec<CommentPart>,
}

impl CommentManipulator {
    pub fn new() -> Self {
        Self { parts: vec![] }
    }

    pub fn push(&mut self, comment: CommentPart) {
        self.parts.push(comment);
    }

    pub fn from_opt_comment(comment: &Option<String>) -> Self {
        CommentManipulator {
            parts: comment
                .as_ref()
                .map(|c| parse_comment(&c))
                .unwrap_or_else(|| Vec::new()),
        }
    }

    pub fn format(&self) -> Option<String> {
        if self.parts.len() > 0 {
            Some(format_comment(&self.parts))
        } else {
            None
        }
    }

    pub fn get_value_tag(&self, find_name: &str) -> Option<(&str)> {
        for part in &self.parts {
            match part {
                CommentPart::ValueTag(name, value) if name == find_name => return Some(value),
                _ => {}
            }
        }
        None
    }

    pub fn has_flag_tag(&self, find_name: &str) -> bool {
        use CommentPart::FlagTag;
        for part in &self.parts {
            match part {
                FlagTag(name) if name == find_name => return true,
                _ => {}
            }
        }
        false
    }

    pub fn remove_flag_tag(&mut self, find_name: &str) {
        use CommentPart::FlagTag;
        self.parts = self
            .parts
            .drain(..)
            .filter(|part| match part {
                FlagTag(name) => name != find_name,
                _ => true,
            })
            .collect();
    }

    pub fn has_value_tag(&self, find_name: &str) -> bool {
        use CommentPart::ValueTag;
        for part in &self.parts {
            match part {
                ValueTag(name, _) if name == find_name => return true,
                _ => {}
            }
        }
        false
    }

    pub fn remove_value_tag(&mut self, find_name: &str) {
        use CommentPart::ValueTag;
        self.parts = self
            .parts
            .drain(..)
            .filter(|part| match part {
                ValueTag(name, _) => name != find_name,
                _ => true,
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_comment() {
        use CommentPart::NewLine;

        let empty: Vec<CommentPart> = vec![];
        assert_eq!(empty, parse_comment(""));
        assert_eq!(
            vec![CommentPart::text("comment text")],
            parse_comment("comment text")
        );
        assert_eq!(
            vec![
                CommentPart::text("start text"),
                NewLine,
                CommentPart::value_tag("key", "value"),
                NewLine,
                CommentPart::text("end text"),
            ],
            parse_comment("start text\nkey: value\nend text"),
        );
        assert_eq!(
            vec![
                CommentPart::text("start text "),
                CommentPart::flag_tag("TAG1"),
                CommentPart::flag_tag("TAG2"),
                CommentPart::text(" end text"),
            ],
            parse_comment("start text :TAG1:TAG2: end text\n"),
        );
        assert_eq!(
            vec![
                CommentPart::text("start text "),
                CommentPart::flag_tag("TAG1"),
                CommentPart::flag_tag("TAG2"),
                CommentPart::text(" end : text : with : colons"),
            ],
            parse_comment("start text :TAG1:TAG2: end : text : with : colons\n"),
        );
        assert_eq!(
            vec![
                CommentPart::text("comment"),
                NewLine,
                CommentPart::flag_tag("flag"),
                CommentPart::text(" ignored-key: value"),
                NewLine,
                CommentPart::value_tag("key", "value"),
            ],
            parse_comment("comment\n:flag: ignored-key: value\nkey: value"),
        );
        assert_eq!(
            vec![
                CommentPart::text("comment"),
                NewLine,
                CommentPart::value_tag("key-without-value", "")
            ],
            parse_comment("comment\nkey-without-value:"),
        );
    }

    #[test]
    fn test_format_comment() {
        use CommentPart::NewLine;
        assert_eq!("", &format_comment(&vec![]));
        assert_eq!(
            "first line\nsecond line",
            &format_comment(&vec![
                CommentPart::text("first line"),
                NewLine,
                CommentPart::text("second line")
            ]),
        );
        assert_eq!(
            "first line\nsecond line\nname: value",
            &format_comment(&vec![
                CommentPart::text("first line"),
                NewLine,
                CommentPart::text("second line"),
                NewLine,
                CommentPart::value_tag("name", "value"),
            ]),
        );
        assert_eq!(
            "text :tag1:tag2: more text :tag3:\n:tag4:",
            &format_comment(&vec![
                CommentPart::text("text "),
                CommentPart::flag_tag("tag1"),
                CommentPart::flag_tag("tag2"),
                CommentPart::text(" more text "),
                CommentPart::flag_tag("tag3"),
                NewLine,
                CommentPart::flag_tag("tag4"),
            ]),
        );
        // Are newlines injected when needed, even if not specified?
        assert_eq!(
            "text :tag1:tag2:\nname1: value1\nmore text :tag3:\nname2: value2",
            &format_comment(&vec![
                CommentPart::text("text "),
                CommentPart::flag_tag("tag1"),
                CommentPart::flag_tag("tag2"),
                CommentPart::value_tag("name1", "value1"),
                CommentPart::text(" more text "),
                CommentPart::flag_tag("tag3"),
                CommentPart::value_tag("name2", "value2"),
            ]),
        );
    }
}
