//! Chunk 1 (PLAN-fael-durable-log): the journal in the git common dir plus the
//! tree + journal union read. Every row lands in the journal first (the commit
//! point of `add`); the tree (`.fael/log`) is the transport on top of it, so a
//! deleted branch, a worktree or a gitignore never takes rows with it.
//!
//! No spawns for the union itself: the common dir is read off `.git` (a dir,
//! or the `gitdir:` pointer of a worktree/submodule), never `git rev-parse` —
//! the read/edit push shares this path and owns a 5 ms ceiling with no spawn
//! in it. Only the `@branch` tags cost one spawn (the current branch), so
//! only `find`/`kickoff` take them.

use crate::find::branches::BranchMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// `<common>/fael` — the journal root all worktrees of this clone share.
/// `None` without git (then the tree is all there is) or when `.git` is
/// unreadable. A linked worktree's pointer ends in `worktrees/<name>`, whose
/// grandparent is the common dir; anything else (submodule, plain dir) is its own.
pub(crate) fn root(repo_root: &Path) -> Option<PathBuf> {
    let dot = repo_root.join(".git");
    if dot.is_dir() {
        return Some(dot.join("fael"));
    }
    let body = std::fs::read_to_string(&dot).ok()?;
    let g = body.strip_prefix("gitdir:")?.trim();
    let g = if Path::new(g).is_absolute() {
        PathBuf::from(g)
    } else {
        repo_root.join(g)
    };
    let common = if g
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "worktrees")
    {
        g.parent()?.parent()?.to_path_buf()
    } else {
        g
    };
    Some(common.join("fael"))
}

/// Tree + journal union (the tree wins on duplicate ids), plus the ids only
/// the journal holds. Spawn-free — the hot paths (`crate::read`) stop here.
fn union(r: &crate::Repo) -> (crate::core::Log, HashSet<String>) {
    let mut log = crate::core::read(&r.fael);
    warn(&log);
    let mut only = HashSet::new();
    let Some(j) = r.journal.as_deref() else {
        return (log, only);
    };
    let extra = crate::core::read(j);
    warn(&extra);
    let mut seen: HashSet<String> = log.rows.iter().map(|x| x.id.clone()).collect();
    for row in extra.rows {
        if seen.insert(row.id.clone()) {
            only.insert(row.id.clone());
            log.rows.push(row);
        }
    }
    let mut seen_c: HashSet<String> = log.closes.iter().map(|x| x.id.clone()).collect();
    for row in extra.closes {
        if seen_c.insert(row.id.clone()) {
            log.closes.push(row);
        }
    }
    (log, only)
}

/// The union read, untagged — what the hooks, the write path and `doctor` use.
pub(crate) fn merged(r: &crate::Repo) -> crate::core::Log {
    union(r).0
}

/// The union read plus the branch each journal-only row was stamped on —
/// unless it is the current branch, where a tag would be noise (a failed tree
/// write on the branch you are on). `find`, `kickoff` and MCP `find` only.
pub(crate) fn read(r: &crate::Repo) -> (crate::core::Log, BranchMap) {
    let (log, only) = union(r);
    if only.is_empty() {
        return (log, BranchMap::new());
    }
    let cur = crate::git(&r.root, &["symbolic-ref", "--short", "-q", "HEAD"]);
    let mut tags = BranchMap::new();
    for row in &log.rows {
        if only.contains(&row.id)
            && let Some(b) = row.branch()
            && Some(b) != cur.as_deref()
        {
            tags.insert(row.id.clone(), b.to_string());
        }
    }
    (log, tags)
}

/// Journal tags under `--branches` tags — keys are disjoint by construction
/// (branch rows merge into the journal union first, HEAD wins), journal wins
/// if they ever meet.
pub(crate) fn overlay(mut jtags: BranchMap, btags: BranchMap) -> BranchMap {
    for (k, v) in btags {
        jtags.entry(k).or_insert(v);
    }
    jtags
}

/// Read-side skew summary, same shape as the old tree-only one in `crate::read`.
fn warn(log: &crate::core::Log) {
    if let Some(first) = log.warnings.first() {
        eprintln!(
            "fael: {} log line(s) skipped — first: {first}",
            log.warnings.len()
        );
    }
}
