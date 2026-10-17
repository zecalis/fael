//! `kickoff` marks a row's `@branch` tag ` (merged)` when that branch's work
//! is already in HEAD — a handoff written there ("uncommitted on feat/x")
//! may be stale. A fact read from git, never a guess about the row's text:
//! the branch tip is merged into HEAD (not merely the commit it was cut
//! from), or — a squash merge — its whole diff from the merge base has the
//! patch-id of a commit on HEAD. Unproven = no mark. Kickoff only: no git
//! spawn belongs on the push path.

use crate::journal::BranchMap;
use fael_core::Row;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

/// Mark the tags of the rows about to be shown; branches of rows off the
/// page are never asked about.
pub fn mark(root: &Path, mut branch_of: BranchMap, rows: &[&Row]) -> BranchMap {
    let common = crate::journal::root(root).and_then(|j| j.parent().map(Path::to_path_buf));
    // Spawn-free dead-branch gate: no live ref under the common dir means no
    // git spawn can prove anything — `is_merged` would fail both `rev-parse`
    // probes and return false. Display-only (decision 01M4HVYM), so skipping
    // keeps rendered bytes identical. Reftable stores no loose/packed refs,
    // so the gate stays off there and git decides.
    let gate = common.filter(|c| !c.join("reftable").exists());
    let packed = gate
        .as_ref()
        .and_then(|c| std::fs::read(c.join("packed-refs")).ok())
        .map(|bytes| {
            // bytes, not a String: one refname with a non-UTF-8 byte would
            // otherwise blind the whole table and read every packed-only ref
            // as dead (git resolves such a name; lossy keeps the other lines).
            let text = String::from_utf8_lossy(&bytes);
            text.lines()
                .filter_map(|l| l.split_once(' '))
                .map(|(_, name)| name.to_string())
                .collect::<std::collections::HashSet<String>>()
        });
    let mut merged: HashMap<String, bool> = HashMap::new();
    for row in rows {
        if let Some(b) = branch_of.get_mut(&row.id) {
            let m = *merged.entry(b.clone()).or_insert_with(|| {
                if gate
                    .as_ref()
                    .is_some_and(|c| !ref_alive(c, packed.as_ref(), b))
                {
                    return false;
                }
                is_merged(root, b)
            });
            if m {
                b.push_str(" (merged)");
            }
        }
    }
    branch_of
}

/// A live ref under the common dir — loose or packed — in any place
/// `git rev-parse` resolves either probe `is_merged` makes, `<b>` and
/// `origin/<b>`: gitrevisions' order is `$GIT_DIR/<p>`, `refs/<p>`,
/// `refs/tags/<p>`, `refs/heads/<p>`, `refs/remotes/<p>`,
/// `refs/remotes/<p>/HEAD`. A name in any of them proves the tip, so missing
/// one loses a real `(merged)` mark — a tag sharing the deleted branch's name
/// (fael:01M52HNB) and a branch named after a pseudo-ref such as FETCH_HEAD
/// (fael:01M52J4N) each did. Pseudo-refs never reach `packed-refs`; a
/// suspicious name falls open (alive) so git decides and bytes stay identical.
fn ref_alive(common: &Path, packed: Option<&std::collections::HashSet<String>>, b: &str) -> bool {
    if b.is_empty() || b.starts_with('/') || b.contains("..") || b.contains('\0') {
        return true;
    }
    let live = |w: &str| common.join(w).is_file() || packed.is_some_and(|set| set.contains(w));
    [b, &format!("origin/{b}")].iter().any(|p| {
        ["", "refs/", "refs/tags/", "refs/heads/", "refs/remotes/"]
            .iter()
            .any(|ns| live(&format!("{ns}{p}")))
            || live(&format!("refs/remotes/{p}/HEAD"))
    })
}

/// Sibling label to `(merged)` for handoff and issue rows whose code files moved since
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
    let changed = crate::hook::files_changed(rows, root, al, prefixes);
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
pub(crate) fn is_merged(root: &Path, b: &str) -> bool {
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

/// Branches git proves merged into HEAD, in three spawns however many branches
/// the repo holds: `for-each-ref --merged` (the ancestor check) minus HEAD's
/// first-parent line (a branch that never left the commit it was cut from has
/// no own commits — not "merged", the `is_merged` rule, batched). A name under
/// `origin/` counts for its short form only when no local branch of that name
/// exists, like `is_merged`'s probe order (local first, else the fetch): a
/// fetched tip merged long ago must not retire a note whose local branch moved
/// on (fael:01M555VZ).
/// Empty when git fails or nothing is merged: unproven, never merged — every
/// caller falls back to `is_merged`, so the squash-merge proof still works.
pub(crate) fn merged_names(root: &Path) -> HashSet<String> {
    let locals: HashSet<String> = git(
        root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .map(|s| s.lines().map(str::to_string).collect())
    .unwrap_or_default();
    let refs = git(
        root,
        &[
            "for-each-ref",
            "--merged",
            "HEAD",
            "--format=%(objectname) %(refname:short)",
            "refs/heads",
            "refs/remotes/origin",
        ],
    )
    .unwrap_or_default();
    let mut tips: Vec<(String, String, bool)> = vec![];
    for line in refs.lines() {
        let Some((tip, name)) = line.split_once(' ') else {
            continue;
        };
        let (short, remote) = match name.strip_prefix("origin/") {
            Some(s) => (s, true),
            None => (name, false),
        };
        if short.is_empty() || short == "HEAD" {
            continue;
        }
        tips.push((tip.to_string(), short.to_string(), remote));
    }
    if tips.is_empty() {
        return HashSet::new();
    }
    let main = git(root, &["rev-list", "--first-parent", "HEAD"]).unwrap_or_default();
    let main: HashSet<&str> = main.lines().collect();
    tips.into_iter()
        .filter(|(tip, _, _)| !main.contains(tip.as_str()))
        .filter(|(_, name, remote)| !remote || !locals.contains(name))
        .map(|(_, name, _)| name)
        .collect()
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
