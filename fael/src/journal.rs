//! Chunk 1 (PLAN-fael-durable-log): the journal in the git common dir plus the
//! tree + journal union read. Every row lands in the journal first (the commit
//! point of `add`); the tree (`.fael/log`) is the transport on top of it, so a
//! deleted branch, a worktree or a gitignore never takes rows with it.
//!
//! No spawns at all: the common dir and the current branch are read off `.git`
//! (a dir, or the `gitdir:` pointer of a worktree/submodule), never `git
//! rev-parse`/`symbolic-ref` — the read/edit push shares this path and owns a
//! 5 ms ceiling with no spawn in it, so the hooks take the `@branch` tags too.

use crate::find::branches::BranchMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// This checkout's git dir — `.git` itself in a plain repo, or the `gitdir:`
/// target of a worktree/submodule's `.git` file. Spawn-free (the read/edit
/// push path must not fork `git`). `None` when `.git` is missing or unreadable.
fn git_dir(repo_root: &Path) -> Option<PathBuf> {
    let dot = repo_root.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let body = std::fs::read_to_string(&dot).ok()?;
    let g = body.strip_prefix("gitdir:")?.trim();
    Some(if Path::new(g).is_absolute() {
        PathBuf::from(g)
    } else {
        repo_root.join(g)
    })
}

/// The checked-out branch, read straight from `<gitdir>/HEAD` — no git spawn.
/// A raw sha (detached HEAD) is `None`; so is a repo without `.git`.
pub(crate) fn head_branch(repo_root: &Path) -> Option<String> {
    let content = std::fs::read_to_string(git_dir(repo_root)?.join("HEAD")).ok()?;
    content
        .strip_prefix("ref:")?
        .trim()
        .strip_prefix("refs/heads/")
        .map(String::from)
}

/// `<common>/fael` — the journal root all worktrees of this clone share.
/// `None` without git (then the tree is all there is) or when `.git` is
/// unreadable. A linked worktree's pointer ends in `worktrees/<name>`, whose
/// grandparent is the common dir; anything else (submodule, plain dir) is its own.
pub(crate) fn root(repo_root: &Path) -> Option<PathBuf> {
    let g = git_dir(repo_root)?;
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

/// The dir whose `log/` holds this clone's rows: the tree's `.fael/` when it
/// has a row file, else the journal (`store = "local"`, or a fresh worktree
/// with no `.fael/` of its own). `None` = fael was never adopted here.
pub(crate) fn home(r: &crate::Repo) -> Option<&Path> {
    let has = |d: &Path| walk_jsonl(&d.join("log")).next().is_some();
    if has(&r.fael) {
        return Some(&r.fael);
    }
    r.journal.as_deref().filter(|j| has(j))
}

/// Any `.jsonl` under `dir` — lazy on purpose: the adopted-here check exits on
/// the first hit instead of walking + sorting the whole log tree the way
/// `core::collect_files` does.
fn walk_jsonl(dir: &Path) -> impl Iterator<Item = PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    std::iter::from_fn(move || {
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "jsonl") {
                    return Some(p);
                }
            }
        }
        None
    })
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
    let cur = head_branch(&r.root);
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
