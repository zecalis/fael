//! Session start: open issues a commit on this branch already names — a fix
//! can land while its issue stays open (01M4XN3S and 01M4XKM4 did, 2026-10-15).

use super::say::{Kind, Line, Outbox};
use crate::core;
use std::path::Path;

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
    let out = crate::git(
        root,
        &[
            "log",
            "--no-merges",
            &format!("--since={since}"),
            "--format=%h%x00%B%x1e",
            "HEAD",
        ],
    )?;
    let mut named: Vec<(String, &str)> = vec![];
    for (sha, body) in out.split('\x1e').filter_map(|c| c.trim().split_once('\0')) {
        for id in super::cited::named_open(log, body) {
            if !named.iter().any(|(n, _)| *n == id) {
                named.push((id, sha));
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
