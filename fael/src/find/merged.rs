//! `kickoff` marks a row's `@branch` tag ` (merged)` when that branch's work
//! is already in HEAD — a handoff written there ("uncommitted on feat/x")
//! may be stale. A fact read from git, never a guess about the row's text:
//! the branch tip is merged into HEAD (not merely the commit it was cut
//! from), or — a squash merge — its whole diff from the merge base has the
//! patch-id of a commit on HEAD. Unproven = no mark. Kickoff only: no git
//! spawn belongs on the push path.

use super::branches::BranchMap;
use fael_core::Row;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

/// Mark the tags of the rows about to be shown; branches of rows off the
/// page are never asked about.
pub fn mark(root: &Path, mut branch_of: BranchMap, rows: &[&Row]) -> BranchMap {
    let mut merged: HashMap<String, bool> = HashMap::new();
    for row in rows {
        if let Some(b) = branch_of.get_mut(&row.id) {
            let m = *merged
                .entry(b.clone())
                .or_insert_with(|| is_merged(root, b));
            if m {
                b.push_str(" (merged)");
            }
        }
    }
    branch_of
}

/// Sibling label to `(merged)` for handoff rows whose code files moved since
/// the row was written (PLAN-fael-file-hash chunk 5a): the same `BranchMap`
/// channel, so whichever plan lands first owns it and the other reuses it.
/// Appended after the branch tag when there is one (`@feat/x (merged) (files
/// changed since)`), else a bare `(files changed since)` the tag renders
/// without `@` — the note's own plan file never counts (it moves every chunk).
pub fn mark_changed(
    root: &Path,
    mut branch_of: BranchMap,
    rows: &[&Row],
    al: &fael_core::Aliases,
    prefixes: &[String],
) -> BranchMap {
    let changed = crate::hook::handoff_changed(rows, root, al, prefixes);
    if changed.is_empty() {
        return branch_of;
    }
    for row in rows.iter().filter(|r| changed.contains(&r.id)) {
        branch_of
            .entry(row.id.clone())
            .and_modify(|b| b.push_str(" (files changed since)"))
            .or_insert_with(|| "(files changed since)".to_string());
    }
    branch_of
}

// ponytail: one patch-id pass over base..HEAD per branch on the page; cache
// by merge base if kickoff ever shows many branches from one old base
fn is_merged(root: &Path, b: &str) -> bool {
    // the local branch, else our fetch of it; a deleted branch proves nothing
    let Some(tip) = ["", "origin/"].iter().find_map(|p| {
        git(
            root,
            &["rev-parse", "--verify", "-q", &format!("{p}{b}^{{commit}}")],
        )
    }) else {
        return false;
    };
    if git(root, &["merge-base", "--is-ancestor", &tip, "HEAD"]).is_some() {
        // on HEAD's first-parent line = the branch never left the commit it
        // was cut from (no own commits) — that is not "merged"
        return git(root, &["rev-list", "--first-parent", "HEAD"])
            .is_some_and(|l| !l.lines().any(|c| c == tip));
    }
    let Some(base) = git(root, &["merge-base", "HEAD", &tip]) else {
        return false;
    };
    let Some(want) = patch_ids(root, &["diff", &base, &tip]).pop() else {
        return false;
    };
    patch_ids(
        root,
        &["log", "-p", "--no-merges", &format!("{base}..HEAD")],
    )
    .contains(&want)
}

/// Trimmed stdout of a git command that exited 0.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `git <args> | git patch-id --stable`: one id per patch in the output.
/// A real pipe, so neither side buffers a whole `log -p` in memory.
fn patch_ids(root: &Path, args: &[&str]) -> Vec<String> {
    let Ok(mut src) = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return vec![];
    };
    let Some(pipe) = src.stdout.take() else {
        return vec![];
    };
    let out = Command::new("git")
        .args(["patch-id", "--stable"])
        .current_dir(root)
        .stdin(pipe)
        .stderr(Stdio::null())
        .output();
    let _ = src.wait();
    let out = out.map(|o| o.stdout).unwrap_or_default();
    String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| l.split(' ').next().map(str::to_string))
        .collect()
}
