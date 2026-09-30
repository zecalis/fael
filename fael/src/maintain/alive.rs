//! Files alive on a row's own branch. A row filed on a branch that has not
//! merged names files the current checkout does not have yet — the work is
//! alive there, not gone, so `[Gone]`/`[PartGone]` must not flag it. One
//! `git ls-tree` per distinct branch, only for rows with a missing file.
//! ponytail: a merged branch still sitting in the clone keeps its files
//! "alive" after main deletes them — `[Merged]` already says to delete it.

use crate::core;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(super) struct BranchFiles<'a> {
    root: &'a Path,
    head: Option<String>,
    trees: RefCell<HashMap<String, Option<HashSet<String>>>>,
}

impl<'a> BranchFiles<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        BranchFiles {
            root,
            head: crate::git(root, &["rev-parse", "--abbrev-ref", "HEAD"]),
            trees: RefCell::new(HashMap::new()),
        }
    }

    /// The row's missing files minus those on its branch's tip (local branch
    /// first, then origin's). A row on HEAD's branch, or with no branch, keeps
    /// every missing file — the checkout is its branch.
    pub(super) fn missing<'r>(&self, row: &'r core::Row, gone: Vec<&'r str>) -> Vec<&'r str> {
        let Some(b) = row.branch().filter(|b| Some(*b) != self.head.as_deref()) else {
            return gone;
        };
        let mut trees = self.trees.borrow_mut();
        let tree = trees.entry(b.to_string()).or_insert_with(|| {
            [b.to_string(), format!("origin/{b}")].iter().find_map(|r| {
                crate::git(self.root, &["ls-tree", "-r", "--name-only", r])
                    .map(|s| s.lines().map(String::from).collect())
            })
        });
        match tree {
            Some(t) => gone.into_iter().filter(|f| !t.contains(*f)).collect(),
            None => gone,
        }
    }
}
