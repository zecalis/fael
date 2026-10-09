//! Whether a closed issue's fix reached this checkout or the default branch
//! (fael:01M4GQVP): a close naming only a branch sha read as fixed while that
//! branch never merged, and carry put a fix in front of agents that main did
//! not have. A squash rewrites every branch sha, so ancestry alone would
//! silence nearly every carry (vela: 0 of 46 sha-only closes are ancestors of
//! main); the squash message keeps each commit's subject, and a commit citing
//! `(fael:<id>)` keeps the id — both are evidence found in git, never a guess.

use std::path::Path;
use std::process::Command;

/// True when `close` names no bare sha (a `(#N)` survives a squash), when a
/// named sha is not in this clone (no evidence either way), or when HEAD or
/// `origin/HEAD` holds a commit carrying one sha's subject — the sha itself
/// when it is an ancestor, the squash that kept its subject otherwise — or
/// naming issue `id`. Two git spawns at most per sha: the edit push budget.
/// ponytail: a generic subject ("wip") matches any commit that repeats it;
/// read PR merge data if that ever lets a stale fix through.
pub(crate) fn fix_reached(root: &Path, close: &str, id: &str) -> bool {
    let cite = &id[..id.len().min(8)];
    let shas = crate::core::stats::bare_shas(close);
    shas.is_empty()
        || shas.iter().any(|sha| {
            let Some(subject) = crate::git(
                root,
                &["log", "-1", "--format=%s", &format!("{sha}^{{commit}}")],
            ) else {
                return true;
            };
            let grep = |refs: &[&str]| {
                let (s, c) = (format!("--grep={subject}"), format!("--grep={cite}"));
                let mut args = vec!["log", "-1", "--format=%h", "-F", &s, &c];
                args.extend(refs);
                Command::new("git")
                    .args(&args)
                    .current_dir(root)
                    .output()
                    .ok()
            };
            // no `origin/HEAD` (no remote, or never set) fails the pair: HEAD alone
            match grep(&["HEAD", "origin/HEAD"]).filter(|o| o.status.success()) {
                Some(o) => !o.stdout.is_empty(),
                None => grep(&["HEAD"]).is_some_and(|o| !o.stdout.is_empty()),
            }
        })
}
