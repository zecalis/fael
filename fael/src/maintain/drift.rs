//! `[Drifted]`: open rows whose files took `DRIFT_COMMITS` or more commits
//! after the row was written. The code moved on, so the row may now restate
//! it (close it: the code says it) or contradict it (supersede it). A fact from
//! git, never a verdict — the reader checks each row against the code. A row
//! checked and still true gets `fael bump <id>` (same text, new version), which
//! restarts its count. One `git log` spawn for the whole log; no git = no note.

use crate::core;
use std::path::Path;

/// ponytail: one fixed threshold; per-repo config if a team's commit rate
/// makes it noisy.
const DRIFT_COMMITS: usize = 10;

/// The `[Drifted]` note, most-moved rows first.
pub(super) fn problem(log: &core::Log, root: &Path) -> Option<core::Problem> {
    let rows: Vec<(&core::Row, i64)> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter_map(|r| Some((r, core::ts_ms(&r.ts)?)))
        .filter(|(r, _)| r.files.iter().any(|f| !crate::hook::is_anchor(f)))
        .collect();
    let oldest = rows.iter().map(|(_, ms)| *ms).min()?;
    let since = format!("@{}", oldest / 1000);
    let out = crate::git(
        root,
        &[
            "log",
            "--format=%x01%ct",
            "--name-only",
            "--since",
            &since,
            "HEAD",
        ],
    )?;
    let commits = parse(&out);
    let mut drifted: Vec<(usize, &core::Row)> = rows
        .into_iter()
        .map(|(r, ms)| (moved(r, ms, &commits), r))
        .filter(|(n, _)| *n >= DRIFT_COMMITS)
        .collect();
    if drifted.is_empty() {
        return None;
    }
    drifted.sort_by_key(|&(n, _)| std::cmp::Reverse(n)); // stable: ties keep log order
    let w = core::abbrev(log);
    let eg: Vec<String> = drifted
        .iter()
        .take(5)
        .map(|(n, r)| format!("{} ×{n} {}", w.short(&r.id), headline(r)))
        .collect();
    Some(
        core::Problem::info(
            core::ProblemKind::Drifted,
            format!(
                "{} open row(s) whose files took {DRIFT_COMMITS}+ commits since they were written \
                 — check each against the code: the code says it now → `fael close <id> \"now in \
                 <file>\"`; wrong now → re-file with `--supersedes <id>`; still true → `fael \
                 bump <id>` restarts the count (e.g. {})",
                drifted.len(),
                eg.join("; ")
            ),
        )
        .with_ids(drifted.iter().map(|(_, r)| r.id.clone()).collect()),
    )
}

/// `git log --format=%x01%ct --name-only` → (commit seconds, paths).
fn parse(out: &str) -> Vec<(i64, Vec<&str>)> {
    out.split('\u{1}')
        .filter_map(|block| {
            let mut lines = block.lines().map(str::trim).filter(|l| !l.is_empty());
            let ct = lines.next()?.parse().ok()?;
            Some((ct, lines.collect()))
        })
        .collect()
}

/// Commits at or after the row's birth second that touch one of its real
/// files (a directory entry covers what sits under it). ponytail: paths are
/// matched as written — a renamed file counts from its new name on only.
fn moved(r: &core::Row, birth_ms: i64, commits: &[(i64, Vec<&str>)]) -> usize {
    let files: Vec<&str> = r
        .files
        .iter()
        .filter(|f| !crate::hook::is_anchor(f))
        .map(|f| f.trim_end_matches('/'))
        .collect();
    let hits = |p: &str| {
        files
            .iter()
            .any(|f| p == *f || p.strip_prefix(f).is_some_and(|t| t.starts_with('/')))
    };
    commits
        .iter()
        .filter(|(ct, paths)| *ct >= birth_ms / 1000 && paths.iter().any(|p| hits(p)))
        .count()
}

/// The row's title, else the start of its text — enough to recognise it.
fn headline(r: &core::Row) -> String {
    let t = r.title.as_deref().unwrap_or(&r.text);
    match t.char_indices().nth(60) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    }
}
