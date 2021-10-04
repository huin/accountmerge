use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::internal::TransactionPostings;
use crate::rules::ctx::PostingContext;
use crate::rules::predicate::Predicate;

const START_CHAIN: &str = "start";

pub fn load_from_path(path: &Path) -> Result<Table> {
    let rf = SourceFile::from_path(path)?;
    let table = rf.load()?;
    table.validate()?;
    Ok(table)
}

#[cfg(test)]
fn load_from_str_unvalidated(s: &str) -> Result<Table> {
    let rf = SourceFile::from_str(s)?;
    let table = rf.load()?;
    Ok(table)
}

#[cfg(test)]
fn load_from_str(s: &str) -> Result<Table> {
    let table = load_from_str_unvalidated(s)?;
    table.validate()?;
    Ok(table)
}

#[derive(Debug)]
struct SourceFile {
    source: Option<PathBuf>,
    entries: Vec<SourceEntry>,
}

impl SourceFile {
    pub fn from_path(path: &Path) -> Result<Self> {
        let entries: Vec<SourceEntry> = ron::de::from_reader(
            File::open(&path).with_context(|| format!("opening {:?} for reading", path))?,
        )
        .with_context(|| format!("parsing {:?}", path))?;
        Ok(SourceFile {
            source: Some(path.to_owned()),
            entries,
        })
    }

    #[cfg(test)]
    pub fn from_str(s: &str) -> Result<Self> {
        let entries: Vec<SourceEntry> = ron::de::from_str(s)?;
        Ok(Self {
            source: None,
            entries,
        })
    }

    fn load(self) -> Result<Table> {
        let mut table = Table::default();
        let mut seen_paths = HashSet::new();
        self.load_into(&mut table, &mut seen_paths)?;
        Ok(table)
    }

    fn load_into(self, table: &mut Table, seen_paths: &mut HashSet<Option<PathBuf>>) -> Result<()> {
        let self_path = self
            .source
            .as_ref()
            .map(std::fs::canonicalize)
            .transpose()
            .with_context(|| format!("canonicalizing path {:?}", self.source))?;
        if !seen_paths.insert(self_path.clone()) {
            // Already loaded.
            return Ok(());
        }

        for entry in self.entries {
            match entry {
                SourceEntry::Include(include_path) => {
                    let include_path = match self_path {
                        Some(ref self_path) => {
                            let parent_dir = self_path.parent().ok_or_else(|| {
                                anyhow!(
                                    "unexpected missing parent directory for path {:?}",
                                    self_path
                                )
                            })?;
                            parent_dir.join(include_path)
                        }
                        None => include_path,
                    };

                    let included_file = Self::from_path(&include_path)?;
                    included_file
                        .load_into(table, seen_paths)
                        .with_context(|| format!("when including from {:?}", include_path))?;
                }
                SourceEntry::Chain(name, rules) => {
                    use std::collections::hash_map::Entry::*;
                    match table.chains.entry(name) {
                        Occupied(entry) => {
                            bail!(
                                "found duplicate definition for chain named {:?}",
                                entry.key()
                            );
                        }
                        Vacant(entry) => {
                            entry.insert(Chain(rules));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
enum SourceEntry {
    Include(PathBuf),
    Chain(String, Vec<Rule>),
}

#[derive(Debug, Default, Deserialize)]
pub struct Table {
    chains: HashMap<String, Chain>,
}

impl Table {
    pub fn update_transactions(
        &self,
        trns: Vec<TransactionPostings>,
    ) -> Result<Vec<TransactionPostings>> {
        trns.into_iter()
            .map(|trn| self.update_transaction(trn))
            .collect::<Result<Vec<TransactionPostings>>>()
    }

    pub fn update_transaction(&self, mut trn: TransactionPostings) -> Result<TransactionPostings> {
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

    fn get_chain(&self, name: &str) -> Result<&Chain> {
        self.chains
            .get(name)
            .ok_or_else(|| anyhow!("chain {} not found", name))
    }

    fn validate(&self) -> Result<()> {
        self.get_chain(START_CHAIN)?;
        for chain in self.chains.values() {
            chain.validate(self)?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
struct Chain(Vec<Rule>);

impl Chain {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<()> {
        for rule in &self.0 {
            match rule.apply(table, ctx)? {
                RuleResult::Continue => {}
                RuleResult::Return => break,
            }
        }
        Ok(())
    }

    fn validate(&self, table: &Table) -> Result<()> {
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
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<RuleResult> {
        if self.predicate.is_match(ctx) {
            self.action.apply(table, ctx)?;
            Ok(self.result)
        } else {
            Ok(RuleResult::Continue)
        }
    }

    fn validate(&self, table: &Table) -> Result<()> {
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
    Error(String),
    Noop,
    JumpChain(String),
    SetAccount(String),
    RemovePostingFlagTag(String),
    RemovePostingValueTag(String),
}

impl Action {
    fn apply(&self, table: &Table, ctx: &mut PostingContext) -> Result<()> {
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
            Error(err_msg) => {
                return Err(anyhow!(
                    "Rule reported error: {}\nWhile processing posting on {}:\n{}",
                    err_msg,
                    ctx.trn.raw.date,
                    ctx.post.raw,
                ));
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

    fn validate(&self, table: &Table) -> Result<()> {
        use Action::*;

        match self {
            JumpChain(name) => table.get_chain(name).map(|_| ()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{format_transaction_postings, parse_transaction_postings};

    #[test]
    fn apply() {
        struct Test {
            name: &'static str,
            table: &'static str,
            cases: Vec<CompiledCase>,
        }
        struct CompiledCase {
            input: Vec<TransactionPostings>,
            want: Vec<TransactionPostings>,
        }
        struct Case {
            input: &'static str,
            want: &'static str,
        }
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
                table: r#"[Chain("start", [])]"#,
                cases: compile_cases(vec![Case {
                    input: r"2001/01/02 description
                        anything  $100.00",
                    want: r"2001/01/02 description
                        anything  $100.00",
                }]),
            },
            Test {
                name: "set account",
                table: r#"[
                    Chain("start", [
                        Rule(action: SetAccount("foo"), predicate: True, result: Continue),
                    ]),
                ]"#,
                cases: compile_cases(vec![Case {
                    input: r"2001/01/02 description
                        anything  $100.00",
                    want: r"2001/01/02 description
                        foo  $100.00",
                }]),
            },
            Test {
                name: "set account in jumped chain",
                table: r#"[
                    Chain("start", [
                        Rule(action: JumpChain("some-chain"), predicate: True, result: Continue),
                    ]),
                    Chain("some-chain", [
                        Rule(action: SetAccount("foo"), predicate: True, result: Continue),
                    ]),
                ]"#,
                cases: compile_cases(vec![Case {
                    input: r"2001/01/02 description
                        anything  $100.00",
                    want: r"2001/01/02 description
                        foo  $100.00",
                }]),
            },
            Test {
                name: "return before set account",
                table: r#"[
                    Chain("start", [
                        Rule(action: Noop, predicate: True, result: Return),
                        Rule(action: SetAccount("foo"), predicate: True, result: Continue),
                    ]),
                ]"#,
                cases: compile_cases(vec![Case {
                    input: r"2001/01/02 description
                        original:value  $100.00",
                    want: r"2001/01/02 description
                        original:value  $100.00",
                }]),
            },
            Test {
                name: "set account based on input account",
                table: r#"[
                    Chain("start", [
                        Rule(action: SetAccount("assets:foo"), predicate: Account(Eq("foo")), result: Continue),
                        Rule(action: SetAccount("assets:bar"), predicate: Account(Eq("bar")), result: Continue),
                    ]),
                ]"#,
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
                table: r#"[
                    Chain("start", [
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
                ]"#,
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
                table: r#"[
                    Chain("start", [
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
                    Chain("set-bank", [
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
                ]"#,
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
                table: r#"[
                    Chain("start", [
                        Rule(
                            action: RemovePostingValueTag("name1"),
                            predicate: True,
                            result: Return,
                        ),
                    ]),
                ]"#,
                cases: compile_cases(vec![Case {
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
                }]),
            },
            Test {
                name: "set based on flag tag",
                table: r#"[
                    Chain("start", [
                        Rule(
                            action: SetAccount("matched"),
                            predicate: PostingHasFlagTag("tag1"),
                            result: Return,
                        ),
                    ]),
                ]"#,
                cases: compile_cases(vec![Case {
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
                }]),
            },
            Test {
                name: "remove flag tag",
                table: r#"[
                    Chain("start", [
                        Rule(
                            action: RemovePostingFlagTag("tag1"),
                            predicate: True,
                            result: Return,
                        ),
                    ]),
                ]"#,
                cases: compile_cases(vec![Case {
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
                }]),
            },
        ];

        for test in &tests {
            let table = load_from_str(test.table)
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
    fn error_action() {
        let table = load_from_str(
            r#"[
                Chain("start", [
                    Rule(
                        action: Error("MY ERROR"),
                        predicate: Account(Eq("bad:account")),
                        result: Return,
                    ),
                ]),
            ]"#,
        )
        .expect("should parse and validate");
        let input = parse_transaction_postings(
            r#"
                2001/01/02 transaction
                    good:account  $10.00
                    bad:account   $-10.00
            "#,
        );
        let got = table.update_transactions(input);
        let err = got.expect_err("wanted an error");
        assert!(err.to_string().contains("MY ERROR"));
        assert!(err.to_string().contains("bad:account"));
    }

    #[test]
    fn validate_valid_tables() {
        struct Test(&'static str, &'static str);
        let tests = vec![
            Test("empty start chain", r#"[Chain("start", [])]"#),
            Test(
                "jump to other chain",
                r#"[
                    Chain("start", [
                        Rule(
                            action: JumpChain("foo"),
                            predicate: True,
                            result: Continue,
                        ),
                    ]),
                    Chain("foo", []),
                ]"#,
            ),
        ];

        for t in &tests {
            let table = load_from_str_unvalidated(t.1).unwrap();
            table
                .validate()
                .expect(&format!("{} => should succeed", t.0));
        }
    }

    #[test]
    fn validate_invalid_tables() {
        struct Test(&'static str, &'static str);
        let tests = vec![
            Test(
                "no start chain",
                r#"[
                        Chain("foo", []),
                    ]"#,
            ),
            Test(
                "jump to non existing chain",
                r#"[
                    Chain("start", [
                        Rule(
                            action: JumpChain("foo"),
                            predicate: True,
                            result: Continue,
                        ),
                    ]),
                    Chain("foo", [
                        Rule(
                            action: JumpChain("not-exist"),
                            predicate: True,
                            result: Continue,
                        ),
                    ]),
                ]"#,
            ),
        ];

        for t in &tests {
            let table = load_from_str_unvalidated(t.1).unwrap();
            table
                .validate()
                .expect_err(&format!("{} => should fail", t.0));
        }
    }
}
