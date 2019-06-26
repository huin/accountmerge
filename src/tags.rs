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

pub fn format_comment(parts: &Vec<CommentPart>) -> String {
    use CommentPart::*;
    let mut out_parts: Vec<String> = Vec::new();
    let mut prev_part: Option<&CommentPart> = None;
    for cur_part in parts {
        match (prev_part, cur_part) {
            // cur_part == FlagTag
            (Some(FlagTag(_)), FlagTag(name)) => out_parts.push(format!("{}:", name)),
            (_, FlagTag(name)) => out_parts.push(format!(":{}:", name)),

            // cur_part == ValueTag
            (None, ValueTag(name, value)) => out_parts.push(format!("{}: {}", name, value)),
            (Some(NewLine), ValueTag(name, value)) => {
                out_parts.push(format!("{}: {}", name, value))
            }
            (Some(_), ValueTag(name, value)) => out_parts.push(format!("\n{}: {}", name, value)),

            // cur_part == Text
            (Some(ValueTag(_, _)), Text(text)) => {
                out_parts.push("\n".to_string());
                out_parts.push(text.to_string());
            }
            (_, Text(text)) => out_parts.push(text.to_string()),

            // cur_part == NewLine
            (None, NewLine) => {}
            (_, NewLine) => out_parts.push("\n".to_string()),
        }
        prev_part = Some(cur_part);
    }
    out_parts.join("")
}

pub fn parse_comment(s: &str) -> Vec<CommentPart> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn flag_tag<S: Into<String>>(name: S) -> CommentPart {
        CommentPart::FlagTag(name.into())
    }

    fn text<S: Into<String>>(text: S) -> CommentPart {
        CommentPart::Text(text.into())
    }

    fn value_tag<S: Into<String>>(name: S, value: S) -> CommentPart {
        CommentPart::ValueTag(name.into(), value.into())
    }

    #[test]
    fn test_parse_comment() {
        use CommentPart::NewLine;

        let empty: Vec<CommentPart> = vec![];
        assert_eq!(empty, parse_comment(""));
        assert_eq!(vec![text("comment text")], parse_comment("comment text"));
        assert_eq!(
            vec![
                text("start text"),
                NewLine,
                value_tag("key", "value"),
                NewLine,
                text("end text"),
            ],
            parse_comment("start text\nkey: value\nend text"),
        );
        assert_eq!(
            vec![
                text("start text "),
                flag_tag("TAG1"),
                flag_tag("TAG2"),
                text(" end text"),
            ],
            parse_comment("start text :TAG1:TAG2: end text\n"),
        );
        assert_eq!(
            vec![
                text("start text "),
                flag_tag("TAG1"),
                flag_tag("TAG2"),
                text(" end : text : with : colons"),
            ],
            parse_comment("start text :TAG1:TAG2: end : text : with : colons\n"),
        );
        assert_eq!(
            vec![
                text("comment"),
                NewLine,
                flag_tag("flag"),
                text(" ignored-key: value"),
                NewLine,
                value_tag("key", "value"),
            ],
            parse_comment("comment\n:flag: ignored-key: value\nkey: value"),
        );
        assert_eq!(
            vec![text("comment"), NewLine, value_tag("key-without-value", "")],
            parse_comment("comment\nkey-without-value:"),
        );
    }

    #[test]
    fn test_format_comment() {
        use CommentPart::NewLine;
        assert_eq!("", &format_comment(&vec![]));
        assert_eq!(
            "first line\nsecond line",
            &format_comment(&vec![text("first line"), NewLine, text("second line")]),
        );
        assert_eq!(
            "first line\nsecond line\nname: value",
            &format_comment(&vec![
                text("first line"),
                NewLine,
                text("second line"),
                NewLine,
                value_tag("name", "value"),
            ]),
        );
        assert_eq!(
            "text :tag1:tag2: more text :tag3:\n:tag4:",
            &format_comment(&vec![
                text("text "),
                flag_tag("tag1"),
                flag_tag("tag2"),
                text(" more text "),
                flag_tag("tag3"),
                NewLine,
                flag_tag("tag4"),
            ]),
        );
        // Are newlines injected when needed, even if not specified?
        assert_eq!(
            "text :tag1:tag2:\nname1: value1\n more text :tag3:\nname2: value2",
            &format_comment(&vec![
                text("text "),
                flag_tag("tag1"),
                flag_tag("tag2"),
                value_tag("name1", "value1"),
                text(" more text "),
                flag_tag("tag3"),
                value_tag("name2", "value2"),
            ]),
        );
    }
}
