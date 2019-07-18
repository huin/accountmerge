use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use failure::Error;
use ledger_parser::{Posting, Transaction};

use crate::tags;

const START_CHAIN: &str = "start";

#[derive(Debug, Fail)]
pub enum RuleError {
    #[fail(display = "chain {} not found", chain)]
    ChainNotFound { chain: String },
}

struct PostingContext<'a> {
    trn: &'a mut Transaction,
    posting_idx: usize,
    posting_comment: tags::CommentLines,
}

impl PostingContext<'_> {
    fn posting(&self) -> &Posting {
        &self.trn.postings[self.posting_idx]
    }

    fn mut_posting(&mut self) -> &mut Posting {
        &mut self.trn.postings[self.posting_idx]
    }
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

    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let reader = File::open(path)?;
        let table: Table = ron::de::from_reader(reader)?;
        table.validate()?;
        Ok(table)
    }

    pub fn update_transaction(&self, trn: &mut Transaction) -> Result<(), RuleError> {
        let start = self.get_chain(START_CHAIN)?;
        for i in 0..trn.postings.len() {
            let pc = tags::CommentLines::from_opt_comment(&trn.postings[i].comment);
            let mut ctx = PostingContext {
                trn: trn,
                posting_idx: i,
                posting_comment: pc,
            };
            start.apply(self, &mut ctx)?;
            ctx.posting_comment.normalize();
            trn.postings[i].comment = ctx.posting_comment.to_opt_comment();
        }
        Ok(())
    }

    fn get_chain(&self, name: &str) -> Result<&Chain, RuleError> {
        self.0.get(name).ok_or_else(|| RuleError::ChainNotFound {
            chain: name.to_string(),
        })
    }

    fn validate(&self) -> Result<(), RuleError> {
        self.get_chain(START_CHAIN)?;
        for (_, chain) in &self.0 {
            chain.validate(self)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
struct Chain(Vec<Rule>);

impl Chain {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<(), RuleError> {
        for rule in &self.0 {
            match rule.apply(table, ctx)? {
                RuleResult::Continue => {}
                RuleResult::Return => break,
            }
        }
        Ok(())
    }

    fn validate(&self, table: &Table) -> Result<(), RuleError> {
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
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<RuleResult, RuleError> {
        if self.predicate.is_match(ctx) {
            self.action.apply(table, ctx)?;
            Ok(self.result)
        } else {
            Ok(RuleResult::Continue)
        }
    }

    fn validate(&self, table: &Table) -> Result<(), RuleError> {
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
    Noop,
    JumpChain(String),
    SetAccount(String),
    RemovePostingFlagTag(String),
    RemovePostingValueTag(String),
}

impl Action {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<(), RuleError> {
        use Action::*;

        match self {
            Noop => {}
            JumpChain(name) => {
                table.get_chain(name)?.apply(table, ctx)?;
            }
            SetAccount(v) => {
                ctx.mut_posting().account = v.clone();
            }
            RemovePostingFlagTag(name) => ctx.posting_comment.remove_flag_tag(&name),
            RemovePostingValueTag(name) => {
                ctx.posting_comment.remove_value_tag(&name);
            }
        }

        Ok(())
    }

    fn validate(&self, table: &Table) -> Result<(), RuleError> {
        use Action::*;

        match self {
            JumpChain(name) => table.get_chain(name).map(|_| ()),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Deserialize)]
enum Predicate {
    True,
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Account(StringMatch),
    PostingHasFlagTag(String),
    PostingHasValueTag(String),
    PostingValueTag(String, StringMatch),
    TransactionDescription(StringMatch),
    Not(Box<Predicate>),
}

impl Predicate {
    fn is_match(&self, ctx: &PostingContext) -> bool {
        use Predicate::*;
        match self {
            True => true,
            All(preds) => preds.iter().all(|p| p.is_match(ctx)),
            Any(preds) => preds.iter().any(|p| p.is_match(ctx)),
            Account(matcher) => matcher.matches_string(&ctx.posting().account),
            PostingHasFlagTag(tag_name) => ctx.posting_comment.has_flag_tag(&tag_name),
            PostingHasValueTag(tag_name) => ctx.posting_comment.has_value_tag(&tag_name),
            PostingValueTag(tag_name, matcher) => ctx
                .posting_comment
                .get_value_tag(&tag_name)
                .map(|value| matcher.matches_string(&value))
                .unwrap_or(false),
            TransactionDescription(matcher) => matcher.matches_string(&ctx.trn.description),
            Not(pred) => !pred.is_match(ctx),
        }
    }
}

#[derive(Debug, Deserialize)]
enum StringMatch {
    Eq(String),
}

impl StringMatch {
    fn matches_string(&self, s: &str) -> bool {
        use StringMatch::*;

        match self {
            Eq(want) => want == s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{format_transactions, parse_transactions};

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
            input: Vec<Transaction>,
            want: Vec<Transaction>,
        };
        struct Case {
            input: &'static str,
            want: &'static str,
        };
        fn compile_cases(cases: Vec<Case>) -> Vec<CompiledCase> {
            cases
                .into_iter()
                .map(|case| CompiledCase {
                    input: parse_transactions(case.input),
                    want: parse_transactions(case.want),
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
                                ; :tag1:tag2: tag in sequence removed
                                ; :tag2: unrelated tag not removed
                            2001/01/03 description2
                                someaccount  $20.00
                        ",
                        want: r"
                            2001/01/02 description1  ; :tag1: transaction tag not matched
                                someaccount  $10.00
                                ; posting tag removed
                                ; :tag2: tag in sequence removed
                                ; :tag2: unrelated tag not removed
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
            for (i, case) in test.cases.iter().enumerate() {
                let mut got = case.input.clone();
                for trn in &mut got {
                    table.update_transaction(trn).unwrap();
                }

                assert_transactions_eq!(
                    case.want,
                    got,
                    "Test \"{}\" case #{}\nFor input:\n{}",
                    test.name,
                    i,
                    format_transactions(&case.input)
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
}
