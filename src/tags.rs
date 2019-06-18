use regex::Regex;

#[derive(Debug, Eq, PartialEq)]
enum CommentPart {
    /// Tag that is present or not, e.g: ":TAG:".
    FlagTag(String),
    /// Tag that has a string value, e.g: "TAG: value".
    ValueTag(String, Option<String>),
    /// Non-tag comment content.
    Text(String),
}

fn parse_comment(s: &str) -> Vec<CommentPart> {
    lazy_static! {
        static ref VALUE_TAG_RX: Regex = Regex::new(r"^[ ]*([^: ]+):(?:[ ]+(.+))?$").unwrap();
    }
    lazy_static! {
        static ref FLAG_TAG_RX: Regex = Regex::new(r":((?:[^: ]+:)+)").unwrap();
    }
    let mut parts = Vec::new();
    for line in s.split('\n') {
        // Value tags comprise an entire comment line.
        if let Some(kv_parts) = VALUE_TAG_RX.captures(line) {
            let key = kv_parts
                .get(1)
                .expect("should always have group 1")
                .as_str();
            let value = kv_parts
                .get(2)
                .expect("should always have group 2")
                .as_str();
            parts.push(CommentPart::ValueTag(
                key.to_string(),
                if value.len() == 0 {
                    None
                } else {
                    Some(value.to_string())
                },
            ));
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

#[test]
fn test_parse_comment() {
    let empty: Vec<CommentPart> = vec![];
    assert_eq!(empty, parse_comment(""));
    assert_eq!(
        vec![CommentPart::Text("comment text".to_string())],
        parse_comment("comment text"),
    );
    assert_eq!(
        vec![
            CommentPart::Text("start text".to_string()),
            CommentPart::ValueTag("key".to_string(), Some("value".to_string())),
            CommentPart::Text("end text".to_string()),
        ],
        parse_comment("start text\nkey: value\nend text"),
    );
    assert_eq!(
        vec![
            CommentPart::Text("start text ".to_string()),
            CommentPart::FlagTag("TAG1".to_string()),
            CommentPart::FlagTag("TAG2".to_string()),
            CommentPart::Text(" end text".to_string()),
        ],
        parse_comment("start text :TAG1:TAG2: end text\n"),
    );
    assert_eq!(
        vec![
            CommentPart::Text("start text ".to_string()),
            CommentPart::FlagTag("TAG1".to_string()),
            CommentPart::FlagTag("TAG2".to_string()),
            CommentPart::Text(" end : text : with : colons".to_string()),
        ],
        parse_comment("start text :TAG1:TAG2: end : text : with : colons\n"),
    );
    assert_eq!(
        vec![
            CommentPart::Text("comment".to_string()),
            CommentPart::FlagTag("flag".to_string()),
            CommentPart::Text(" ignored-key: value".to_string()),
            CommentPart::ValueTag("key".to_string(), Some("value".to_string())),
        ],
        parse_comment("comment\n:flag: ignored-key: value\nkey: value"),
    );
}
