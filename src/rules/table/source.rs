use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_derive::Deserialize;

use crate::rules::table::{Chain, Rule, Table};

#[derive(Debug)]
pub struct File {
    source: Option<PathBuf>,
    entries: Vec<Entry>,
}

impl File {
    pub fn from_path(path: &Path) -> Result<Self> {
        let entries: Vec<Entry> = ron::de::from_reader(
            std::fs::File::open(path).with_context(|| format!("opening {:?} for reading", path))?,
        )
        .with_context(|| format!("parsing {:?}", path))?;
        Ok(File {
            source: Some(path.to_owned()),
            entries,
        })
    }

    #[cfg(test)]
    pub fn from_str(s: &str) -> Result<Self> {
        let entries: Vec<Entry> = ron::de::from_str(s)?;
        Ok(Self {
            source: None,
            entries,
        })
    }

    pub fn load(self) -> Result<Table> {
        let mut chains = HashMap::<String, Chain>::new();
        let mut seen_paths = HashSet::new();
        self.load_into(&mut chains, &mut seen_paths)?;
        Ok(Table::new(chains))
    }

    fn load_into(
        self,
        chains: &mut HashMap<String, Chain>,
        seen_paths: &mut HashSet<Option<PathBuf>>,
    ) -> Result<()> {
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
                Entry::Include(include_path) => {
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
                        .load_into(chains, seen_paths)
                        .with_context(|| format!("when including from {:?}", include_path))?;
                }
                Entry::Chain(name, rules) => {
                    use std::collections::hash_map::Entry::*;
                    match chains.entry(name) {
                        Occupied(entry) => {
                            bail!(
                                "found duplicate definition for chain named {:?}",
                                entry.key()
                            );
                        }
                        Vacant(entry) => {
                            entry.insert(Chain::new(rules));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
enum Entry {
    Include(PathBuf),
    Chain(String, Vec<Rule>),
}
