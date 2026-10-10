//! Whether a closed issue's fix reached this checkout or the default branch
//! (fael:01M4GQVP): a close naming only a branch sha read as fixed while that
//! branch never merged, and carry put a fix in front of agents that main did
//! not have. A close at or after the cutoff (`stats::new_close`) is judged by
//! what git keeps under every merge style — a commit citing `(fael:<prefix>)`
//! or the close's `(#N)` (`stats::cites_fix`, fael:01M4HTZ4) — never a sha.
//! An older close keeps its rule: a squash rewrites every branch sha, so
//! ancestry alone would silence nearly every carry (vela: 0 of 46 sha-only
//! closes are ancestors of main); the squash message keeps each commit's
//! subject, and a commit naming the id keeps the id.
//!
//! "Main" is HEAD and `origin/HEAD`, HEAD alone when `origin/HEAD` is unset,
//! as the local refs stand: nothing is fetched on the hook path.

use std::path::Path;
use std::process::Command;

/// `Some(true)` reached, `Some(false)` not, `None` unknown — git could read
/// no main ref for a new close (`new`), which is never reported as "not
/// reached". A new close is one spawn, two when `origin/HEAD` is unset; an
/// old one two per named sha at most: the edit push budget. An old close
/// keeps its rule whole: reached when it names no bare sha, a named sha is
/// not in this clone, or main holds that sha's subject or the id (missing
/// refs read as not reached, as the chunk 1 baseline counted them).
/// ponytail: a generic subject ("wip") matches any commit that repeats it;
/// only old closes read subjects, so this ceiling no longer grows.
pub(crate) fn fix_reached(
    root: &Path,
    log: &crate::core::Log,
    id: &str,
    close: &str,
    new: bool,
) -> Option<bool> {
    let cite = &id[..id.len().min(8)];
    if new {
        let mut args = vec!["--format=%B%x00".to_string(), "-F".into()];
        args.push(format!("--grep=(fael:{cite}"));
        args.extend(crate::core::stats::pr_cites(close).map(|p| format!("--grep={p}")));
        let out = on_main(root, &args)?;
        let ids = || log.rows.iter().map(|r| r.id.as_str());
        let is_id = |p: &str| crate::core::stats::resolve(p, ids()) == Some(id);
        return Some(
            out.split('\0')
                .any(|m| crate::core::stats::cites_fix(m, close, is_id)),
        );
    }
    let shas = crate::core::stats::bare_shas(close);
    Some(
        shas.is_empty()
            || shas.iter().any(|sha| {
                let Some(subject) = crate::git(
                    root,
                    &["log", "-1", "--format=%s", &format!("{sha}^{{commit}}")],
                ) else {
                    return true;
                };
                let args = ["-1", "--format=%h", "-F"].map(String::from);
                let greps = [format!("--grep={subject}"), format!("--grep={cite}")];
                on_main(root, &[&args[..], &greps[..]].concat()).is_some_and(|o| !o.is_empty())
            }),
    )
}

/// `git log <args>` over HEAD and `origin/HEAD`, else HEAD alone (no remote,
/// or never set); `None` when neither reads.
fn on_main(root: &Path, args: &[String]) -> Option<String> {
    let log = |refs: &[&str]| {
        let o = Command::new("git")
            .arg("log")
            .args(args)
            .args(refs)
            .current_dir(root)
            .output()
            .ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    log(&["HEAD", "origin/HEAD"]).or_else(|| log(&["HEAD"]))
}
