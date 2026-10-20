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
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

// A proof is a pure function of object ids (tip, HEAD, merge base), so these
// memos are exact for the life of the process — a branch ref that moves
// changes the tip sha and misses. `add decision` runs the proof twice (the cap
// check, then the held text) and a repo with many open branches repeats one
// `log -p base..HEAD` per tip (fael:01M5CPNJ). No HEAD sha (reftable) = no
// memo, git decides every time.
type Verdicts = HashMap<(PathBuf, String, String, bool), bool>;
type LogIds = HashMap<(PathBuf, String, String), Vec<String>>;
static VERDICTS: LazyLock<Mutex<Verdicts>> = LazyLock::new(Default::default);
static LOG_IDS: LazyLock<Mutex<LogIds>> = LazyLock::new(Default::default);
// ponytail: dropped wholesale past this many entries; an LRU if a long-lived
// `fael mcp` ever churns through more distinct HEADs than this
const MEMO_MAX: usize = 256;

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

pub(crate) fn is_merged(root: &Path, b: &str) -> bool {
    // the local branch, else our fetch of it; a deleted branch proves nothing
    match live_tip(root, b) {
        Some(tip) => merged_tip(root, &tip, true),
        None => false,
    }
}

/// `is_merged` for a note whose branch ref may be gone: the live ref wins; else the
/// tip sha the row was stamped with, if its commit is still in the object store —
/// proved by patch-id only (the stamped tip may predate later commits on the branch,
/// so "is an ancestor of HEAD" would prove too much). No ref, no sha = unproven.
pub(crate) fn is_merged_at(root: &Path, b: &str, sha: Option<&str>) -> bool {
    if let Some(tip) = live_tip(root, b) {
        return merged_tip(root, &tip, true);
    }
    let Some(sha) = sha.filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_hexdigit()))
    else {
        return false;
    };
    git(
        root,
        &["rev-parse", "--verify", "-q", &format!("{sha}^{{commit}}")],
    )
    .is_some_and(|tip| merged_tip(root, &tip, false))
}

fn live_tip(root: &Path, b: &str) -> Option<String> {
    ["", "origin/"].iter().find_map(|p| {
        git(
            root,
            &["rev-parse", "--verify", "-q", &format!("{p}{b}^{{commit}}")],
        )
    })
}

fn merged_tip(root: &Path, tip: &str, ancestor_proves: bool) -> bool {
    let Some(head) = crate::journal::head_sha(root) else {
        return prove_tip(root, tip, None, ancestor_proves);
    };
    let key = (
        root.to_path_buf(),
        tip.to_string(),
        head.clone(),
        ancestor_proves,
    );
    if let Some(v) = VERDICTS.lock().ok().and_then(|m| m.get(&key).copied()) {
        return v;
    }
    let v = prove_tip(root, tip, Some(&head), ancestor_proves);
    if let Ok(mut m) = VERDICTS.lock() {
        if m.len() >= MEMO_MAX {
            m.clear();
        }
        m.insert(key, v);
    }
    v
}

fn prove_tip(root: &Path, tip: &str, head: Option<&str>, ancestor_proves: bool) -> bool {
    let tip = tip.to_string();
    if ancestor_proves && git(root, &["merge-base", "--is-ancestor", &tip, "HEAD"]).is_some() {
        // on HEAD's first-parent line = the branch never left the commit it
        // was cut from (no own commits) — that is not "merged"
        return git(root, &["rev-list", "--first-parent", "HEAD"])
            .is_some_and(|l| !l.lines().any(|c| c == tip));
    }
    if git(root, &["merge-base", "--is-ancestor", &tip, "HEAD"]).is_some() {
        return false;
    }
    let Some(base) = git(root, &["merge-base", "HEAD", &tip]) else {
        return false;
    };
    let Some(want) = patch_ids(root, &["diff", &base, &tip]).pop() else {
        return false;
    };
    head_patch_ids(root, &base, head).contains(&want)
}

/// The patch-ids of `base..HEAD`, once per (base, HEAD) however many tips
/// share the merge base.
fn head_patch_ids(root: &Path, base: &str, head: Option<&str>) -> Vec<String> {
    let log = || {
        patch_ids(
            root,
            &["log", "-p", "--no-merges", &format!("{base}..HEAD")],
        )
    };
    let Some(head) = head else { return log() };
    let key = (root.to_path_buf(), base.to_string(), head.to_string());
    if let Some(v) = LOG_IDS.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return v;
    }
    let v = log();
    if let Ok(mut m) = LOG_IDS.lock() {
        if m.len() >= MEMO_MAX {
            m.clear();
        }
        m.insert(key, v.clone());
    }
    v
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

#[cfg(test)]
thread_local!(static SPAWNED: std::cell::RefCell<Vec<String>> = Default::default());

/// Trimmed stdout of a git command that exited 0.
fn git(root: &Path, args: &[&str]) -> Option<String> {
    #[cfg(test)]
    SPAWNED.with(|s| s.borrow_mut().push(args.join(" ")));
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
    #[cfg(test)]
    SPAWNED.with(|s| s.borrow_mut().push(args.join(" ")));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn g(d: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(d)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn commit_file(d: &Path, name: &str, msg: &str) {
        std::fs::write(d.join(name), name).unwrap();
        g(d, &["add", name]);
        g(d, &["commit", "-qm", msg]);
    }

    fn spawns(what: &str) -> usize {
        SPAWNED.with(|s| s.borrow().iter().filter(|a| a.starts_with(what)).count())
    }

    /// Three branches off one base, two of them squash-merged onto main: the
    /// verdicts are the same cold and memoised, the `log -p base..HEAD` pass
    /// runs once, and the second pass proves nothing again (fael:01M5CPNJ).
    #[test]
    fn proofs_are_memoised_and_unchanged() {
        let d = std::env::temp_dir().join(format!("fael-merged-memo-{}", fael_core::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let d = d.canonicalize().unwrap();
        g(&d, &["init", "-q", "-b", "main"]);
        g(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
        for b in ["x", "y", "z"] {
            g(&d, &["switch", "-qc", b, "main"]);
            commit_file(&d, b, "branch");
        }
        g(&d, &["switch", "-q", "main"]);
        // the squash: the same diff as a fresh commit on main, no ancestry
        // a different message: the same second would otherwise hash to the
        // branch's own commit and make it an ancestor, not a squash
        commit_file(&d, "x", "squash");
        commit_file(&d, "y", "squash");
        let pass = || ["x", "y", "z"].map(|b| is_merged_at(&d, b, None));
        let cold = pass();
        assert_eq!(cold, [true, true, false]);
        assert_eq!(
            spawns("log -p"),
            1,
            "{:?}",
            SPAWNED.with(|s| s.borrow().clone())
        );
        let (logs, diffs, bases) = (spawns("log -p"), spawns("diff"), spawns("merge-base HEAD"));
        assert_eq!(pass(), cold);
        assert_eq!(
            (spawns("log -p"), spawns("diff"), spawns("merge-base HEAD")),
            (logs, diffs, bases),
            "the second pass proves nothing again"
        );
        // a moved HEAD is a new key: the proof runs again and still agrees
        commit_file(&d, "z", "squash");
        assert_eq!(pass(), [true, true, true]);
    }
}
