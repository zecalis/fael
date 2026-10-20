//! `kickoff` marks a row's `@branch` tag ` (merged)` when that branch's work
//! is already in HEAD — a handoff written there ("uncommitted on feat/x")
//! may be stale. A fact read from git, never a guess about the row's text:
//! the branch tip is merged into HEAD (not merely the commit it was cut
//! from), or — a squash merge — its whole diff from the merge base has the
//! patch-id of a commit on HEAD. Unproven = no mark. Kickoff only: no git
//! spawn belongs on the push path.

use crate::journal::BranchMap;
use fael_core::Row;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

/// Mark the tags of the rows about to be shown; branches of rows off the
/// page are never asked about.
pub fn mark(root: &Path, mut branch_of: BranchMap, rows: &[&Row]) -> BranchMap {
    let common = crate::journal::root(root).and_then(|j| j.parent().map(Path::to_path_buf));
    // Spawn-free dead-branch gate: no live ref under the common dir means no
    // git spawn can prove anything — the proof would find no tip for either
    // probe and return false. Display-only (decision 01M4HVYM), so skipping
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
    // each branch the rows name, once; one pull answers them all (`view`)
    let mut names: Vec<&str> = vec![];
    for b in rows.iter().filter_map(|r| branch_of.get(&r.id)) {
        let alive = gate
            .as_ref()
            .is_none_or(|c| ref_alive(c, packed.as_ref(), b));
        if alive && !names.contains(&b.as_str()) {
            names.push(b);
        }
    }
    let items: Vec<(&str, Option<&str>)> = names.iter().map(|b| (*b, None)).collect();
    let merged: HashMap<String, bool> = names
        .iter()
        .map(|b| b.to_string())
        .zip(super::view::prove(root, &items))
        .collect();
    for row in rows {
        if let Some(b) = branch_of.get_mut(&row.id)
            && merged.get(b.as_str()) == Some(&true)
        {
            b.push_str(" (merged)");
        }
    }
    branch_of
}

/// A live ref under the common dir — loose or packed — in any place
/// `git rev-parse` resolves either probe the proof makes, `<b>` and
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

#[cfg(test)]
thread_local!(pub(super) static SPAWNED: std::cell::RefCell<Vec<String>> = Default::default());

/// Trimmed stdout of a git command that exited 0.
pub(super) fn git(root: &Path, args: &[&str]) -> Option<String> {
    git_out(root, args)
        .filter(|(code, _)| *code == Some(0))
        .map(|(_, out)| out.trim().to_string())
}

/// Exit code and stdout of a git command; `None` when git cannot spawn.
pub(super) fn git_out(root: &Path, args: &[&str]) -> Option<(Option<i32>, String)> {
    #[cfg(test)]
    SPAWNED.with(|s| s.borrow_mut().push(args.join(" ")));
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some((
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
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
    /// verdicts are the same cold, memoised and from the disk cache, the
    /// `log -p` pass runs once, and the second pass proves nothing again
    /// (fael:01M5CPNJ, 01M5CRPS).
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
        let pass = || super::super::view::prove(&d, &[("x", None), ("y", None), ("z", None)]);
        let bases = || spawns("rev-list --parents --boundary");
        let cold = pass();
        assert_eq!(cold, [true, true, false]);
        assert_eq!(
            spawns("log -p"),
            1,
            "{:?}",
            SPAWNED.with(|s| s.borrow().clone())
        );
        // the three tips share one merge-base walk, and no tip spawns its own
        assert_eq!((bases(), spawns("merge-base")), (1, 0));
        let (logs, diffs, all) = (
            spawns("log -p"),
            spawns("diff"),
            SPAWNED.with(|s| s.borrow().len()),
        );
        assert_eq!(pass(), cold);
        assert_eq!(
            SPAWNED.with(|s| s.borrow().len()),
            all,
            "the same pull proves nothing again"
        );
        // a new pull (view gone) reads the on-disk caches: same verdicts, no
        // diff, no log, no merge-base — only the ref listings are read again
        super::super::view::forget();
        assert_eq!(pass(), cold);
        assert_eq!(
            (spawns("log -p"), spawns("diff"), bases()),
            (logs, diffs, 1)
        );
        // a moved HEAD: only the new commit is diffed, and the ids still
        // equal a plain `log -p base..HEAD | patch-id` over the whole range
        commit_file(&d, "z", "squash");
        assert_eq!(pass(), [true, true, true]);
        assert_eq!(spawns("log -p"), logs + 1);
        let base = git(&d, &["rev-parse", "HEAD~3"]).unwrap();
        let plain = Command::new("sh")
            .current_dir(&d)
            .args([
                "-c",
                &format!("git log -p --no-merges {base}..HEAD | git patch-id --stable"),
            ])
            .output()
            .unwrap();
        let plain: std::collections::HashSet<String> = String::from_utf8_lossy(&plain.stdout)
            .lines()
            .map(|l| l.split(' ').next().unwrap().to_string())
            .collect();
        let head = git(&d, &["rev-parse", "HEAD"]).unwrap();
        let cached = super::super::patch_ids::head_ids(&d, &head, std::slice::from_ref(&base));
        assert_eq!(cached[&base], plain);
    }
}
