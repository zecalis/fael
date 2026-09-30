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
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// This checkout's git dir — `.git` itself in a plain repo, or the `gitdir:`
/// target of a worktree/submodule's `.git` file. Spawn-free (the read/edit
/// push path must not fork `git`). `None` when `.git` is missing or unreadable.
pub(crate) fn git_dir(repo_root: &Path) -> Option<PathBuf> {
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

/// Whether `refs/heads/<branch>` still exists in the clone — a loose ref file
/// or a `packed-refs` line, read off `<common>`, no git spawn.
fn branch_alive(common: &Path, branch: &str) -> bool {
    if common.join("refs/heads").join(branch).is_file() {
        return true;
    }
    let want = format!("refs/heads/{branch}");
    std::fs::read_to_string(common.join("packed-refs")).is_ok_and(|p| {
        p.lines()
            .any(|l| l.split_once(' ').is_some_and(|(_, name)| name == want))
    })
}

/// The union read plus the branch each row was stamped on when that branch is
/// not the current one — `find`, `kickoff`, MCP `find` and the hooks.
/// - a journal-only row always carries its tag (the change may exist nowhere
///   else; the branch may even be deleted);
/// - a tree row carries it only when the tree is shared — `.fael` a symlink
///   across worktrees — and that branch still exists. A tree that follows
///   `git switch` holds only this branch's history, so a tag there would be
///   noise (a merged branch); a shared tree holds every branch's rows and
///   reads as fact on a branch that lacks the change. Once the branch is gone
///   (merged, or a throwaway worktree branch) the tag points at nothing.
///
/// Detached HEAD has no branch to compare, so tree rows go untagged there.
// ponytail: a merged branch that still exists is tagged in a shared tree
// (ancestry needs a git spawn); an untracked non-symlink tree is not covered
// — `store = "local"` already makes those rows journal-only.
pub(crate) fn read(r: &crate::Repo) -> (crate::core::Log, BranchMap) {
    let (log, only) = union(r);
    let cur = head_branch(&r.root);
    let common = r.journal.as_deref().and_then(Path::parent);
    let shared = std::fs::symlink_metadata(&r.fael).is_ok_and(|m| m.file_type().is_symlink());
    let mut alive: HashMap<&str, bool> = HashMap::new();
    let mut tags = BranchMap::new();
    for row in &log.rows {
        let Some(b) = row.branch().filter(|b| Some(*b) != cur.as_deref()) else {
            continue;
        };
        let keep = only.contains(&row.id)
            || (shared
                && cur.is_some()
                && common.is_some_and(|c| *alive.entry(b).or_insert_with(|| branch_alive(c, b))));
        if keep {
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
