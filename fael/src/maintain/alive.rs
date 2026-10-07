//! Files alive on a row's own branch. A row filed on a branch that has not
//! merged names files the current checkout does not have yet — the work is
//! alive there, not gone, so `[Gone]`/`[PartGone]` must not flag it. One
//! `git ls-tree` per row with a missing file, over just those paths — never the
//! whole tree; each (branch, path) is asked once.
//! ponytail: a merged branch still sitting in the clone keeps its files
//! "alive" after main deletes them — `[Merged]` already says to delete it.

use crate::core;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(crate) struct BranchFiles<'a> {
    root: &'a Path,
    head: Option<String>,
    /// `(branch, path)` → alive on that branch's tip; each path is asked once.
    alive: RefCell<HashMap<(String, String), bool>>,
}

impl<'a> BranchFiles<'a> {
    pub(crate) fn new(root: &'a Path) -> Self {
        BranchFiles {
            root,
            head: crate::git(root, &["rev-parse", "--abbrev-ref", "HEAD"]),
            alive: RefCell::new(HashMap::new()),
        }
    }

    /// The row's missing files minus those on its branch's tip (local branch
    /// first, then origin's). A row on HEAD's branch, or with no branch, keeps
    /// every missing file — the checkout is its branch.
    pub(crate) fn missing<'r>(&self, row: &'r core::Row, gone: Vec<&'r str>) -> Vec<&'r str> {
        let Some(b) = row.branch().filter(|b| Some(*b) != self.head.as_deref()) else {
            return gone;
        };
        let mut alive = self.alive.borrow_mut();
        let key = |f: &str| (b.to_string(), f.to_string());
        let ask: Vec<&str> = gone
            .iter()
            .copied()
            .filter(|f| !alive.contains_key(&key(f)))
            .collect();
        if !ask.is_empty() {
            let found = self.on_tip(b, &ask);
            for f in ask {
                alive.insert(key(f), found.contains(f));
            }
        }
        gone.into_iter().filter(|f| !alive[&key(f)]).collect()
    }

    /// Which of `files` exist on `b`'s tip: one `ls-tree` over just those
    /// paths, never the whole tree.
    fn on_tip(&self, b: &str, files: &[&str]) -> HashSet<String> {
        [b.to_string(), format!("origin/{b}")]
            .iter()
            .find_map(|r| {
                let mut args = vec!["ls-tree", "-r", "--name-only", r.as_str(), "--"];
                args.extend(files);
                crate::git(self.root, &args)
            })
            .map(|s| s.lines().map(String::from).collect())
            .unwrap_or_default()
    }
}
