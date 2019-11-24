use std::collections::HashMap;

use failure::Error;

use crate::filespec::FileSpec;
use crate::internal::{PostingInternal, TransactionInternal, TransactionPostings};

const START_CHAIN: &str = "start";

#[derive(Debug, Fail)]
pub enum RuleError {
    #[fail(display = "chain {} not found", chain)]
    ChainNotFound { chain: String },
}

struct PostingContext<'a> {
    trn: &'a mut TransactionInternal,
    post: &'a mut PostingInternal,
}

#[derive(Debug, Default, Deserialize)]
pub struct Table(HashMap<String, Chain>);

impl Table {
    #[cfg(test)]
    pub fn from_str(s: &str) -> Result<Self, Error> {
        let table: Table = ron::de::from_str(s)?;
        table.validate()?;
        Ok(table)
    }

    pub fn from_filespec(file_spec: &FileSpec) -> Result<Self, Error> {
        let reader = file_spec.reader()?;
        let table: Table = ron::de::from_reader(reader)?;
        table.validate()?;
        Ok(table)
    }

    pub fn update_transactions(
        &self,
        trns: Vec<TransactionPostings>,
    ) -> Result<Vec<TransactionPostings>, Error> {
        trns.into_iter()
            .map(|trn| self.update_transaction(trn))
            .collect::<Result<Vec<TransactionPostings>, Error>>()
    }

    pub fn update_transaction(
        &self,
        mut trn: TransactionPostings,
    ) -> Result<TransactionPostings, Error> {
        let start = self.get_chain(START_CHAIN)?;
        for post in &mut trn.posts {
            let mut ctx = PostingContext {
                trn: &mut trn.trn,
                post,
            };
            start.apply(self, &mut ctx)?;
        }
        Ok(trn)
    }

    fn get_chain(&self, name: &str) -> Result<&Chain, Error> {
        self.0.get(name).ok_or_else(|| {
            RuleError::ChainNotFound {
                chain: name.to_string(),
            }
            .into()
        })
    }

    fn validate(&self) -> Result<(), Error> {
        self.get_chain(START_CHAIN)?;
        for chain in self.0.values() {
            chain.validate(self)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
struct Chain(Vec<Rule>);

impl Chain {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<(), Error> {
        for rule in &self.0 {
            match rule.apply(table, ctx)? {
                RuleResult::Continue => {}
                RuleResult::Return => break,
            }
        }
        Ok(())
    }

    fn validate(&self, table: &Table) -> Result<(), Error> {
        for r in &self.0 {
            r.validate(table)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct Rule {
    predicate: Predicate,
    action: Action,
    result: RuleResult,
}

impl Rule {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<RuleResult, Error> {
        if self.predicate.is_match(ctx) {
            self.action.apply(table, ctx)?;
            Ok(self.result)
        } else {
            Ok(RuleResult::Continue)
        }
    }

    fn validate(&self, table: &Table) -> Result<(), Error> {
        self.action.validate(table)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum RuleResult {
    Continue,
    Return,
}

#[derive(Debug, Deserialize)]
enum Action {
    AddPostingFlagTag(String),
    All(Vec<Action>),
    Noop,
    JumpChain(String),
    SetAccount(String),
    RemovePostingFlagTag(String),
    RemovePostingValueTag(String),
}

impl Action {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<(), Error> {
        use Action::*;

        match self {
            AddPostingFlagTag(name) => {
                ctx.post.comment.tags.insert(name.to_string());
            }
            All(actions) => {
                for action in actions {
                    action.apply(table, ctx)?;
                }
            }
            Noop => {}
            JumpChain(name) => {
                table.get_chain(name)?.apply(table, ctx)?;
            }
            SetAccount(v) => {
                ctx.post.raw.account = v.clone();
            }
            RemovePostingFlagTag(name) => {
                ctx.post.comment.tags.remove(name);
            }
            RemovePostingValueTag(name) => {
                ctx.post.comment.value_tags.remove(name);
            }
        }

        Ok(())
    }

    fn validate(&self, table: &Table) -> Result<(), Error> {
        use Action::*;

        match self {
            JumpChain(name) => table.get_chain(name).map(|_| ()),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Deserialize)]
enum Predicate {
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Account(StringMatch),
    PostingHasFlagTag(String),
    PostingHasValueTag(String),
    PostingValueTag(String, StringMatch),
    Not(Box<Predicate>),
    TransactionDescription(StringMatch),
    True,
}

impl Predicate {
    fn is_match(&self, ctx: &PostingContext) -> bool {
        use Predicate::*;
        match self {
            True => true,
            All(preds) => preds.iter().all(|p| p.is_match(ctx)),
            Any(preds) => preds.iter().any(|p| p.is_match(ctx)),
            Account(matcher) => matcher.matches_string(&ctx.post.raw.account),
            Not(pred) => !pred.is_match(ctx),
            PostingHasFlagTag(tag_name) => ctx.post.comment.tags.contains(tag_name),
            PostingHasValueTag(tag_name) => ctx.post.comment.value_tags.contains_key(tag_name),
            PostingValueTag(tag_name, matcher) => ctx
                .post
                .comment
                .value_tags
                .get(tag_name)
                .map(|value| matcher.matches_string(&value))
                .unwrap_or(false),
            TransactionDescription(matcher) => matcher.matches_string(&ctx.trn.raw.description),
        }
    }

    #[cfg(test)]
    pub fn from_str(s: &str) -> Result<Self, Error> {
        ron::de::from_str(s).map_err(Into::into)
    }
}

#[derive(Debug, Deserialize)]
enum StringMatch {
    AsLower(Box<StringMatch>),
    Contains(String),
    Eq(String),
}

impl StringMatch {
    fn matches_string(&self, s: &str) -> bool {
        use StringMatch::*;

        match self {
            AsLower(m) => m.matches_string(&s.to_lowercase()),
            Contains(want) => s.contains(want),
            Eq(want) => want == s,
        }
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;
    use crate::testutil::{format_transaction_postings, parse_transaction_postings};

    struct TableBuilder {
        table: Table,
    }
    impl TableBuilder {
        fn new() -> Self {
            TableBuilder {
                table: Table::default(),
            }
        }

        fn chain(mut self, name: &str, chain: Chain) -> Self {
            self.table.0.insert(name.to_string(), chain);
            self
        }

        fn build(self) -> Table {
            self.table
        }
    }

    struct ChainBuilder {
        chain: Chain,
    }
    impl ChainBuilder {
        fn new() -> Self {
            ChainBuilder {
                chain: Chain::default(),
            }
        }

        fn rule(mut self, action: Action, predicate: Predicate, result: RuleResult) -> Self {
            self.chain.0.push(Rule {
                action,
                predicate,
                result,
            });
            self
        }

        fn build(self) -> Chain {
            self.chain
        }
    }

    fn jump_chain(chain: &str) -> Action {
        Action::JumpChain(chain.to_string())
    }

    #[test]
    fn apply() {
        struct Test {
            name: &'static str,
            table: &'static str,
            cases: Vec<CompiledCase>,
        };
        struct CompiledCase {
            input: Vec<TransactionPostings>,
            want: Vec<TransactionPostings>,
        };
        struct Case {
            input: &'static str,
            want: &'static str,
        };
        fn compile_cases(cases: Vec<Case>) -> Vec<CompiledCase> {
            cases
                .into_iter()
                .map(|case| CompiledCase {
                    input: parse_transaction_postings(case.input),
                    want: parse_transaction_postings(case.want),
                })
                .collect()
        }

        let tests = vec![
            Test {
                name: "empty chain",
                table: r#"Table ({"start": Chain([]) })"#,
                cases: compile_cases(vec![Case {
                    input: r"2001/01/02 description
                        anything  $100.00",
                    want: r"2001/01/02 description
                        anything  $100.00",
                }]),
            },
            Test {
                name: "set account",
                table: r#"Table ({
                        "start": Chain([
                            Rule(action: SetAccount("foo"), predicate: True, result: Continue),
                        ]),
                    })"#,
                cases: compile_cases(vec![Case {
                    input: r"2001/01/02 description
                        anything  $100.00",
                    want: r"2001/01/02 description
                        foo  $100.00",
                }]),
            },
            Test {
                name: "set account in jumped chain",
                table: r#"Table ({
                        "start": Chain([
                            Rule(action: JumpChain("some-chain"), predicate: True, result: Continue),
                        ]),
                        "some-chain": Chain([
                            Rule(action: SetAccount("foo"), predicate: True, result: Continue),
                        ]),
                    })"#,
                cases: compile_cases(vec![Case {
                    input: r"2001/01/02 description
                        anything  $100.00",
                    want: r"2001/01/02 description
                        foo  $100.00",
                }]),
            },
            Test {
                name: "return before set account",
                table: r#"Table ({
                        "start": Chain([
                            Rule(action: Noop, predicate: True, result: Return),
                            Rule(action: SetAccount("foo"), predicate: True, result: Continue),
                        ]),
                    })"#,
                cases: compile_cases(vec![Case {
                    input: r"2001/01/02 description
                        original:value  $100.00",
                    want: r"2001/01/02 description
                        original:value  $100.00",
                }]),
            },
            Test {
                name: "set account based on input account",
                table: r#"Table ({
                        "start": Chain([
                            Rule(action: SetAccount("assets:foo"), predicate: Account(Eq("foo")), result: Continue),
                            Rule(action: SetAccount("assets:bar"), predicate: Account(Eq("bar")), result: Continue),
                        ]),
                    })"#,
                cases: compile_cases(vec![
                    Case {
                        input: r"2001/01/02 description
                            foo  $100.00",
                        want: r"2001/01/02 description
                            assets:foo  $100.00",
                    },
                    Case {
                        input: r"2001/01/02 description
                            bar  $100.00",
                        want: r"2001/01/02 description
                            assets:bar  $100.00",
                    },
                    Case {
                        input: r"2001/01/02 description
                            quux  $100.00",
                        want: r"2001/01/02 description
                            quux  $100.00",
                    },
                ]),
            },
            Test {
                name: "set account based on various boolean conditions",
                table: r#"Table ({
                        "start": Chain([
                            Rule(
                                action: SetAccount("assets:acct1-bank1"),
                                predicate: All([
                                    Account(Eq("acct1")),
                                    TransactionDescription(Eq("bank1")),
                                ]),
                                result: Return,
                            ),
                            Rule(
                                action: SetAccount("assets:acct1-bank2"),
                                predicate: All([
                                    Account(Eq("acct1")),
                                    TransactionDescription(Eq("bank2")),
                                ]),
                                result: Return,
                            ),
                            Rule(
                                action: SetAccount("assets:acct2-bank1"),
                                predicate: All([
                                    Account(Eq("acct2")),
                                    TransactionDescription(Eq("bank1")),
                                ]),
                                result: Return,
                            ),
                            Rule(
                                action: SetAccount("assets:acct2-bank2"),
                                predicate: All([
                                    Account(Eq("acct2")),
                                    TransactionDescription(Eq("bank2")),
                                ]),
                                result: Return,
                            ),
                            Rule(
                                action: SetAccount("assets:acct3-or-4"),
                                predicate: Any([
                                    Account(Eq("acct3")),
                                    Account(Eq("acct4")),
                                ]),
                                result: Return,
                            ),
                            Rule(
                                action: SetAccount("assets:acct-other-bank1"),
                                predicate: All([
                                    Not(Account(Eq("acct1"))),
                                    Not(Account(Eq("acct2"))),
                                    TransactionDescription(Eq("bank1")),
                                ]),
                                result: Return,
                            ),
                        ]),
                    })"#,
                cases: compile_cases(vec![
                    Case {
                        input: r"2001/01/02 unmatched
                            acct1  $10.00",
                        want: r"2001/01/02 unmatched
                            acct1  $10.00",
                    },
                    Case {
                        input: r"2001/01/02 bank1
                            acct1  $10.00",
                        want: r"2001/01/02 bank1
                            assets:acct1-bank1  $10.00",
                    },
                    Case {
                        input: r"2001/01/02 bank1
                            acct2  $10.00",
                        want: r"2001/01/02 bank1
                            assets:acct2-bank1  $10.00",
                    },
                    Case {
                        input: r"2001/01/02 bank2
                            acct2  $10.00",
                        want: r"2001/01/02 bank2
                            assets:acct2-bank2  $10.00",
                    },
                    Case {
                        input: r"2001/01/02 description
                            acct3  $10.00",
                        want: r"2001/01/02 description
                            assets:acct3-or-4  $10.00",
                    },
                    Case {
                        input: r"2001/01/02 description
                            assets:acct3-or-4  $10.00",
                        want: r"2001/01/02 description
                            assets:acct3-or-4  $10.00",
                    },
                    Case {
                        input: r"2001/01/02 bank1
                            acct5  $10.00",
                        want: r"2001/01/02 bank1
                            assets:acct-other-bank1  $10.00",
                    },
                ]),
            },
            Test {
                name: "set bank based on tag value",
                table: r#"Table({
                        "start": Chain([
                            Rule(
                                action: JumpChain("set-bank"),
                                predicate: PostingHasValueTag("bank"),
                                result: Return,
                            ),
                            Rule(
                                action: SetAccount("assets:unknown"),
                                predicate: True,
                                result: Return,
                            ),
                        ]),
                        "set-bank": Chain([
                            Rule(
                                action: SetAccount("assets:bank:foo"),
                                predicate: PostingValueTag("bank", Eq("foo")),
                                result: Return,
                            ),
                            Rule(
                                action: SetAccount("assets:bank:bar"),
                                predicate: PostingValueTag("bank", Eq("bar")),
                                result: Return,
                            ),
                            Rule(
                                action: SetAccount("assets:bank:other"),
                                predicate: True,
                                result: Return,
                            ),
                        ]),
                    })"#,
                cases: compile_cases(vec![
                    Case {
                        input: r"2001/01/02 description
                            someaccount  $10.00
                            ; bank: foo",
                        want: r"2001/01/02 description
                            assets:bank:foo  $10.00
                            ; bank: foo",
                    },
                    Case {
                        input: r"2001/01/02 description
                            someaccount  $10.00
                            ; bank: bar",
                        want: r"2001/01/02 description
                            assets:bank:bar  $10.00
                            ; bank: bar",
                    },
                    Case {
                        input: r"2001/01/02 description
                            someaccount  $10.00
                            ; bank: quux",
                        want: r"2001/01/02 description
                            assets:bank:other  $10.00
                            ; bank: quux",
                    },
                    Case {
                        input: r"2001/01/02 description
                            someaccount  $10.00",
                        want: r"2001/01/02 description
                            assets:unknown  $10.00",
                    },
                ]),
            },
            Test {
                name: "remove value tag",
                table: r#"Table({
                    "start": Chain([
                        Rule(
                            action: RemovePostingValueTag("name1"),
                            predicate: True,
                            result: Return,
                        ),
                    ]),
                })"#,
                cases: compile_cases(vec![
                    Case {
                        input: r"
                            2001/01/02 description1  ; name1: transaction tag not removed
                                someaccount  $10.00
                                ; name1: posting tag removed
                                ; name2: unrelated tag not removed
                            2001/01/03 description2
                                someaccount  $20.00
                        ",
                        want: r"
                            2001/01/02 description1  ; name1: transaction tag not removed
                                someaccount  $10.00
                                ; name2: unrelated tag not removed
                            2001/01/03 description2
                                someaccount  $20.00
                        ",
                    },
                ]),
            },
            Test {
                name: "set based on flag tag",
                table: r#"Table({
                    "start": Chain([
                        Rule(
                            action: SetAccount("matched"),
                            predicate: PostingHasFlagTag("tag1"),
                            result: Return,
                        ),
                    ]),
                })"#,
                cases: compile_cases(vec![
                    Case {
                        input: r"
                            2001/01/02 description1
                                someaccount  $10.00
                                ; :tag1: posting tag matches
                            2001/01/03 description2  ; :tag1: transaction tag not matched
                                someaccount  $20.00
                        ",
                        want: r"
                            2001/01/02 description1
                                matched  $10.00
                                ; :tag1: posting tag matches
                            2001/01/03 description2  ; :tag1: transaction tag not matched
                                someaccount  $20.00
                        ",
                    },
                ]),
            },
            Test {
                name: "remove flag tag",
                table: r#"Table({
                    "start": Chain([
                        Rule(
                            action: RemovePostingFlagTag("tag1"),
                            predicate: True,
                            result: Return,
                        ),
                    ]),
                })"#,
                cases: compile_cases(vec![
                    Case {
                        input: r"
                            2001/01/02 description1  ; :tag1: transaction tag not matched
                                someaccount  $10.00
                                ; :tag1: posting tag removed
                                ; :tag2: unrelated tag not removed
                            2001/01/03 description2
                                someaccount  $20.00
                        ",
                        want: r"
                            2001/01/02 description1  ; :tag1: transaction tag not matched
                                someaccount  $10.00
                                ; :tag2: posting tag removed
                                ; unrelated tag not removed
                            2001/01/03 description2
                                someaccount  $20.00
                        ",
                    },
                ]),
            },
        ];

        for test in &tests {
            let table = Table::from_str(test.table)
                .expect(&format!("failed to parse table for test {}", test.name));
            for (case_idx, case) in test.cases.iter().enumerate() {
                let got = table
                    .update_transactions(case.input.clone())
                    .expect("update_transactions");

                assert_transaction_postings_eq!(
                    case.want.clone(),
                    got,
                    "Test \"{}\" case #{}\nFor input:\n{}",
                    test.name,
                    case_idx,
                    format_transaction_postings(case.input.clone())
                );
            }
        }
    }

    #[test]
    fn validate_valid_tables() {
        use RuleResult::*;
        struct Test(&'static str, Table);
        let tests = vec![
            Test(
                "empty start chain",
                TableBuilder::new().chain("start", Chain::default()).build(),
            ),
            Test(
                "jump to other chain",
                TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(jump_chain("foo"), Predicate::True, Continue)
                            .build(),
                    )
                    .chain("foo", Chain::default())
                    .build(),
            ),
        ];

        for t in &tests {
            t.1.validate().expect(&format!("{} => should succeed", t.0));
        }
    }

    #[test]
    fn validate_invalid_tables() {
        use RuleResult::*;
        struct Test(&'static str, Table);
        let tests = vec![
            Test(
                "no start chain",
                TableBuilder::new().chain("foo", Chain::default()).build(),
            ),
            Test(
                "jump to non existing chain",
                TableBuilder::new()
                    .chain(
                        "start",
                        ChainBuilder::new()
                            .rule(jump_chain("foo"), Predicate::True, Continue)
                            .build(),
                    )
                    .chain(
                        "foo",
                        ChainBuilder::new()
                            .rule(jump_chain("not-exist"), Predicate::True, Continue)
                            .build(),
                    )
                    .build(),
            ),
        ];

        for t in &tests {
            t.1.validate()
                .expect_err(&format!("{} => should fail", t.0));
        }
    }

    const SIMPLE_POSTING: &str = r#"
        2000/01/01 Transaction description
            account:name  $10.00
            ; :flag-tag:
            ; value-tag: value-tag-value
            ; shouty-key: SHOUTY-VALUE
    "#;

    #[test_case("Account(Contains(\"name\"))", SIMPLE_POSTING => true)]
    #[test_case("Account(Contains(\"other\"))", SIMPLE_POSTING => false)]
    #[test_case("Account(Eq(\"account:name\"))", SIMPLE_POSTING => true)]
    #[test_case("Account(Eq(\"account:other\"))", SIMPLE_POSTING => false)]
    #[test_case("Not(True)", SIMPLE_POSTING => false)]
    #[test_case("PostingHasFlagTag(\"flag-tag\")", SIMPLE_POSTING => true)]
    #[test_case("PostingHasFlagTag(\"other-flag-tag\")", SIMPLE_POSTING => false)]
    #[test_case("PostingHasValueTag(\"value-tag\")", SIMPLE_POSTING => true)]
    #[test_case("PostingHasValueTag(\"other-value-tag\")", SIMPLE_POSTING => false)]
    #[test_case("PostingValueTag(\"value-tag\", Eq(\"value-tag-value\"))", SIMPLE_POSTING => true)]
    #[test_case("PostingValueTag(\"value-tag\", Eq(\"other-value-tag-value\"))", SIMPLE_POSTING => false)]
    #[test_case("PostingValueTag(\"other-value-tag\", Eq(\"value-tag-value\"))", SIMPLE_POSTING => false)]
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
