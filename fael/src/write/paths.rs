//! `--files` read from a subdirectory: the cwd reading first, the repo-root
//! reading when only that one exists.

use crate::{core, hook};
use fael_core::sibling_suggest;
use std::collections::HashSet;
use std::path::Path;

/// `fael add` from `sub/` with `--files sub/a.rs` (repo-root-relative) reads as `sub/sub/a.rs`;
/// when that is missing but the root reading exists, take it — a warning only when the arg does
/// not name `sub/` itself. The cwd reading wins whenever it exists (`a.rs` keeps `sub/a.rs`).
pub(super) fn root_relative(r: &crate::Repo, args: &[String], files: &mut [String]) -> Vec<String> {
    let mut warns = vec![];
    let sub = r.cwd.strip_prefix(&r.root).unwrap_or(&r.cwd);
    for (arg, f) in args.iter().zip(files.iter_mut()) {
        if hook::is_anchor(f) || core::is_glob(f) || r.root.join(&*f).exists() {
            continue;
        }
        if let Ok(v) = core::normalize_files(std::slice::from_ref(arg), &r.root, &r.root)
            && let Some(alt) = v.into_iter().next()
            && r.root.join(&alt).exists()
        {
            if !Path::new(arg).starts_with(sub) {
                warns.push(format!(
                    "warning: {arg:?} is not under {sub:?} — resolved from repo root as {alt:?}"
                ));
            }
            *f = alt;
        }
    }
    warns
}

/// Check a files list against evidence before the row is written. Accepted
/// silently, in order: anchor · glob · on disk · rename-resolvable · in this
/// session's edits · in git status (shell-made files the edit hook never saw).
///
/// A path with no evidence is rejected only when almost surely a typo (a
/// same-directory file within edit distance 2); anything else files with a
/// warning — deleted or not-yet-created files are legitimate rows. `force`
/// turns the rejection into a warning.
pub(crate) fn check(
    root: &Path,
    al: &core::Aliases,
    files: &[String],
    edits: &[(String, i64)],
    force: bool,
) -> Result<Vec<String>, String> {
    let in_edits: HashSet<&str> = edits.iter().map(|(p, _)| p.as_str()).collect();
    let mut missing: Vec<&str> = vec![];
    for f in files {
        if hook::is_anchor(f) || core::is_glob(f) || root.join(f).exists() {
            continue;
        }
        if al.forward(f).iter().any(|p| root.join(p).exists()) {
            continue;
        }
        if in_edits.contains(f.as_str()) {
            continue;
        }
        missing.push(f.as_str());
    }
    if missing.is_empty() {
        return Ok(vec![]);
    }
    // one spawn, only when something is missing from disk — the add path
    // already spawns git for the stamp, so this costs nothing on success
    let status = git_status_files(root);
    missing.retain(|f| !status.contains(*f));
    let mut bad: Vec<(&str, String)> = vec![];
    let mut warns: Vec<String> = vec![];
    for f in missing {
        match sibling_suggest(root, f) {
            Some(near) if !force => bad.push((f, near)),
            _ => warns.push(format!(
                "warning: {f:?} matches nothing on disk — filed anyway; a typo? else its rows push once it exists"
            )),
        }
    }
    if !bad.is_empty() {
        let (f, near) = &bad[0];
        let mut msg = format!(
            "rejected: {f:?} matches nothing — not on disk, no rename leads to it, \
not in this session's edits or git status — did you mean {near:?}? \
(a file you have not created yet: add --force)"
        );
        if bad.len() > 1 {
            msg.push_str(&format!(" (+{} more)", bad.len() - 1));
        }
        return Err(msg);
    }
    Ok(warns)
}

/// Paths git knows in the worktree but the disk walk above missed: untracked,
/// staged and modified files (`git mv` without commit stages the new name).
/// Empty on any failure — fail-open, the caller just has less evidence.
fn git_status_files(root: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(s) = crate::git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    ) else {
        return out;
    };
    // `-z` entries are `XY <path>`; a rename or copy (`R`/`C` in X) is
    // followed by its source as a bare entry — the new name is the evidence
    let mut it = s.split('\0');
    while let Some(e) = it.next() {
        let Some(p) = e.get(3..).filter(|p| !p.is_empty()) else {
            continue;
        };
        if e.starts_with(['R', 'C']) {
            it.next();
        }
        out.insert(p.to_string());
    }
    out
}
