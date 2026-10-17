//! Session start: open issues a commit on this branch already names — a fix
//! can land while its issue stays open (01M4XN3S and 01M4XKM4 did, 2026-10-15).

use super::say::{Kind, Line, Outbox};
use crate::core;
use std::path::Path;

/// Repo-relative prefix: a file is under a scope dir, or the same path.
fn under(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim_end_matches('/'), b.trim_end_matches('/'));
    a == b
        || a.strip_prefix(b).is_some_and(|r| r.starts_with('/'))
        || b.strip_prefix(a).is_some_and(|r| r.starts_with('/'))
}

/// At most this many named issues get a line — the rest are one count.
const NAMED_MAX: usize = 3;

/// Say the open issues a commit on HEAD names (the `commit_cites` rule)
/// since the oldest of them was filed, as one Cited line. The commit names the id; whether it fixed it is the agent's call. One
/// `git log` at session start; no git or no commit = no line.
// ponytail: HEAD only — a branch cut before the fix misses it; add
// origin/HEAD if that miss shows up in use
pub(super) fn say(out: &mut Outbox, root: &Path, log: &core::Log, all: &[&core::Row]) {
    if let Some((ids, text)) = line(root, log, all) {
        out.say(Line {
            kind: Kind::Cited { ids },
            text,
        });
    }
}

fn line(root: &Path, log: &core::Log, all: &[&core::Row]) -> Option<(Vec<String>, String)> {
    let since = all
        .iter()
        .filter(|r| r.kind == "issue")
        .map(|r| r.ts.as_str())
        .min()?;
    // only commits unique to this branch: a fix already on the trunk
    // (origin/HEAD) is not "on this branch" — `git log origin/main..HEAD`
    // holds just this branch's work. No origin (fresh test repos): HEAD as before.
    let base = crate::git(root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]);
    // on the trunk the branch-only range is empty: nothing unique to warn about.
    if let Some(b) = base.as_deref()
        && let Some(head) = crate::git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        && Some(head.as_str()) == b.strip_prefix("origin/")
    {
        return None;
    }
    let range: Option<String> = base.as_deref().map(|b| format!("{b}..HEAD"));
    let log_range = range.clone().unwrap_or_else(|| "HEAD".to_string());
    let out = crate::git(
        root,
        &[
            "log",
            "--no-merges",
            &format!("--since={since}"),
            "--format=%h%x00%B%x1e",
            &log_range,
        ],
    )?;
    // the issue's files still touched here: with a trunk to diff against,
    // a commit naming an unrelated flake (evidence.ts) must not fire on a
    // StartCard branch. No trunk: no file check (old behaviour).
    let branch_files: Vec<String> = range
        .as_deref()
        .and_then(|r| {
            let b = r.strip_suffix("..HEAD")?;
            crate::git(root, &["diff", "--name-only", &format!("{b}...HEAD")])
        })
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    let mut named: Vec<(String, &str)> = vec![];
    for (sha, body) in out.split('\x1e').filter_map(|c| c.trim().split_once('\0')) {
        for id in super::cited::named_open(log, body) {
            if !named.iter().any(|(n, _)| *n == id) {
                // the issue's files still touched here: a commit naming an
                // unrelated flake (evidence.ts) must not fire on a StartCard branch.
                let touches = range.is_none()
                    || log.rows.iter().find(|r| r.id == id).is_some_and(|r| {
                        r.files.is_empty()
                            || r.files
                                .iter()
                                .any(|f| branch_files.iter().any(|b| under(f, b) || under(b, f)))
                    });
                if touches {
                    named.push((id, sha));
                }
            }
        }
    }
    let ab = core::abbrev(log);
    let mut text: String = named
        .iter()
        .take(NAMED_MAX)
        .map(|(id, sha)| {
            let id = ab.short(id);
            format!("fael: {id} is still open, and commit {sha} on this branch names it — fixed? fael close {id} \"<why>\" (MCP close)\n")
        })
        .collect();
    if named.len() > NAMED_MAX {
        text.push_str(&format!(
            "fael: +{} more open issues named in commits\n",
            named.len() - NAMED_MAX
        ));
    }
    let ids = named
        .into_iter()
        .take(NAMED_MAX)
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    (!ids.is_empty()).then_some((ids, text))
}
