//! `[Drifted]`: open rows whose files took `DRIFT_COMMITS` or more commits
//! after the row was written. The code moved on, so the row may now restate
//! it (close it: the code says it) or contradict it (supersede it). A fact from
//! git, never a verdict — the reader checks each row against the code. A row
//! checked and still true gets `fael bump <id>` (same id, restamped), which
//! restarts its count. One `git log` spawn for the whole log; no git = no note.

use crate as core;
use std::path::Path;

/// ponytail: one fixed threshold; per-repo config if a team's commit rate
/// makes it noisy.
const DRIFT_COMMITS: usize = 10;

/// The `[Drifted]` note, most-moved rows first.
pub fn problem(log: &core::Log, root: &Path) -> Option<core::Problem> {
    let mut drifted = drifted(log, root);
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

/// Full ids of the drifted rows — the board's `drifted` signal (PLAN-fael-board l1).
pub fn ids(log: &core::Log, root: &Path) -> std::collections::HashSet<String> {
    drifted(log, root)
        .into_iter()
        .map(|(_, r)| r.id.clone())
        .collect()
}

/// Every open row at `DRIFT_COMMITS`+ commits, in log order, with its count.
fn drifted<'a>(log: &'a core::Log, root: &Path) -> Vec<(usize, &'a core::Row)> {
    let rows: Vec<(&core::Row, i64)> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter_map(|r| Some((r, core::ts_ms(&r.ts)?)))
        .filter(|(r, _)| r.files.iter().any(|f| !crate::is_anchor(f)))
        .collect();
    let Some(oldest) = rows.iter().map(|(_, ms)| *ms).min() else {
        return vec![];
    };
    let since = format!("@{}", oldest / 1000);
    let Some(out) = crate::repo::git(
        root,
        &[
            "log",
            "--format=%x01%ct",
            "--name-only",
            "--since",
            &since,
            "HEAD",
        ],
    ) else {
        return vec![];
    };
    let commits = parse(&out);
    rows.into_iter()
        .map(|(r, ms)| (moved(r, ms, &commits), r))
        .filter(|(n, _)| *n >= DRIFT_COMMITS)
        .collect()
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
/// files (a directory entry covers what sits under it). A row that names code
/// counts its code only: docs like `docs/architecture.md` move with nearly
/// every PR and would drift every decision filed against them. ponytail: paths
/// are matched as written — a renamed file counts from its new name on only.
fn moved(r: &core::Row, birth_ms: i64, commits: &[(i64, Vec<&str>)]) -> usize {
    let mut files: Vec<&str> = r
        .files
        .iter()
        .filter(|f| !crate::is_anchor(f))
        .map(|f| f.trim_end_matches('/'))
        .collect();
    let is_doc = |f: &&str| f.ends_with(".md");
    if files.iter().any(|f| !is_doc(f)) {
        files.retain(|f| !is_doc(f));
    }
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

/// One commit of `log_since`: commit second, subject, touched paths.
pub struct Commit {
    pub ct: i64,
    pub subject: String,
    pub paths: Vec<String>,
}

/// Newest-first commits since `since_ms` touching any of `files`: one scoped
/// `git log` for the named rows, never one per row. No git = empty.
pub fn log_since(root: &Path, since_ms: i64, files: &[&str]) -> Vec<Commit> {
    if files.is_empty() {
        return vec![];
    }
    let since = format!("@{}", since_ms / 1000);
    let mut args = vec![
        "log",
        "--format=%x01%ct%x02%s",
        "--name-only",
        "--since",
        &since,
        "HEAD",
        "--",
    ];
    args.extend(files);
    let Some(out) = crate::repo::git(root, &args) else {
        return vec![];
    };
    out.split('\u{1}')
        .filter_map(|block| {
            let mut lines = block.lines().map(str::trim).filter(|l| !l.is_empty());
            let (ct, subject) = lines.next()?.split_once('\u{2}')?;
            Some(Commit {
                ct: ct.parse().ok()?,
                subject: subject.to_string(),
                paths: lines.map(String::from).collect(),
            })
        })
        .collect()
}

/// The row's title, else the start of its text — enough to recognise it.
fn headline(r: &core::Row) -> String {
    let t = r.title.as_deref().unwrap_or(&r.text);
    match t.char_indices().nth(60) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    }
}

/// One drifted row with what the sweep needs: the file that moved most since the
/// row was written, its commit count, and the row's total — one `git log` for
/// every row, parsed once. Code files only (docs counted only when the row
/// names no code), same rule as `moved`.
pub fn detail(log: &core::Log, root: &Path) -> Vec<(core::Row, String, usize, usize)> {
    let rows: Vec<(&core::Row, i64)> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter_map(|r| Some((r, core::ts_ms(&r.ts)?)))
        .filter(|(r, _)| r.files.iter().any(|f| !crate::is_anchor(f)))
        .collect();
    let Some(oldest) = rows.iter().map(|(_, ms)| *ms).min() else {
        return vec![];
    };
    let since = format!("@{}", oldest / 1000);
    let Some(out) = crate::repo::git(
        root,
        &[
            "log",
            "--format=%x01%ct",
            "--name-only",
            "--since",
            &since,
            "HEAD",
        ],
    ) else {
        return vec![];
    };
    let commits = parse(&out);
    let mut out = vec![];
    for (r, ms) in rows {
        // code files only (docs counted only when the row names no code), same rule as `moved`
        let mut files: Vec<&str> = r
            .files
            .iter()
            .filter(|f| !crate::is_anchor(f))
            .map(|f| f.trim_end_matches('/'))
            .collect();
        if files.iter().any(|f| !f.ends_with(".md")) {
            files.retain(|f| !f.ends_with(".md"));
        }
        let mut per: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut total = 0;
        for (ct, paths) in &commits {
            if *ct < ms / 1000 {
                continue;
            }
            let mut hit: Vec<&str> = vec![];
            for p in paths {
                if files
                    .iter()
                    .any(|f| *p == *f || p.strip_prefix(f).is_some_and(|t| t.starts_with('/')))
                {
                    hit.push(p);
                }
            }
            if hit.is_empty() {
                continue;
            }
            total += 1;
            for p in hit {
                *per.entry(p.to_string()).or_default() += 1;
            }
        }
        if total < DRIFT_COMMITS {
            continue;
        }
        let (top, n) = per
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(f, n)| (f.to_string(), n))
            .unwrap_or_default();
        let top = if top.is_empty() {
            r.files.first().cloned().unwrap_or_default()
        } else {
            top
        };
        out.push(((*r).clone(), top, n, total));
    }
    out.sort_by_key(|(_, _, _, total)| std::cmp::Reverse(*total));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_count_only_when_the_row_names_no_code() {
        let row = |files: &[&str]| core::Row {
            files: files.iter().map(|f| f.to_string()).collect(),
            ..Default::default()
        };
        let commits = vec![(1, vec!["docs/a.md"]), (1, vec!["src/x.rs"])];
        assert_eq!(moved(&row(&["docs/a.md", "src/x.rs"]), 0, &commits), 1);
        assert_eq!(moved(&row(&["docs/a.md"]), 0, &commits), 1);
        assert_eq!(moved(&row(&["docs/", "src/x.rs"]), 0, &commits), 2);
    }
}
