//! Facts under a held row in `chunk done` / `chunk start` (issue 01M5EEGR):
//! which stamped files changed and which still match, the commits on the
//! changed file since the row, its bumps, and the command to look. Never a
//! verdict — fael states what it counted, the agent judges. One scoped
//! `git log` for all named rows, pull path only (never the push line).

use super::changed::{Blobs, STAMP_MAX_BYTES, file_verdict, resolve};
use crate::core;
use std::path::Path;

const SUBJECTS: usize = 2;

/// Evidence lines for `rows[i]` at index `i` (a row with its top file), each
/// at most three lines: files, commits + bumps, inspect.
pub(crate) fn lines(
    log: &core::Log,
    root: &Path,
    al: &core::Aliases,
    rows: &[(&core::Row, String)],
) -> Vec<String> {
    let since: Vec<i64> = rows.iter().map(|(r, _)| since_ms(log, r)).collect();
    let top: Vec<&str> = rows.iter().map(|(_, f)| f.as_str()).collect();
    let commits = match since.iter().min() {
        Some(&min) => core::drift::log_since(root, min, &top),
        None => vec![],
    };
    let mut blobs = Blobs::new();
    rows.iter()
        .zip(&since)
        .map(|((r, f), &since)| {
            let mut out = String::new();
            if let Some(l) = files_line(r, root, al, &mut blobs) {
                out += &format!("  {l}\n");
            }
            let mine: Vec<_> = commits
                .iter()
                .filter(|c| c.ct >= since / 1000 && c.paths.iter().any(|p| p == f))
                .collect();
            let bumps = log
                .rows
                .iter()
                .filter(|b| b.bumps.as_deref() == Some(&r.id))
                .count();
            let subjects: Vec<String> = mine
                .iter()
                .take(SUBJECTS)
                .map(|c| format!("\"{}\"", clip(&c.subject)))
                .collect();
            let tail = if subjects.is_empty() {
                String::new()
            } else {
                format!(": {}", subjects.join("; "))
            };
            out += &format!(
                "  {} commit(s) on `{f}` since the row was last written{tail} · bumped {bumps}×\n",
                mine.len()
            );
            let id = core::abbrev(log).short(&r.id);
            out += &format!("  inspect: fael find {id} && git log --oneline -5 -- {f}\n");
            out
        })
        .collect()
}

/// The row's last bump, else its birth: the point its stamp dates from.
fn since_ms(log: &core::Log, r: &core::Row) -> i64 {
    let last = log
        .rows
        .iter()
        .filter(|b| b.bumps.as_deref() == Some(&r.id))
        .filter_map(|b| core::ulid_ms(&b.id))
        .max();
    last.map(|m| m as i64)
        .or_else(|| core::ts_ms(&r.ts))
        .unwrap_or(0)
}

/// `3/4 files changed, still matches: a.ts` over the stamped files with a
/// verdict (a gone, unreadable or over-cap file has none and is not counted).
fn files_line(r: &core::Row, root: &Path, al: &core::Aliases, blobs: &mut Blobs) -> Option<String> {
    let (mut changed, mut same) = (0, vec![]);
    for (f, v) in r.file_hashes()? {
        let (Some(want), Some(t)) = (v.as_str(), resolve(al, root, f)) else {
            continue;
        };
        match file_verdict(want, &t, root, blobs, STAMP_MAX_BYTES) {
            Some(true) => changed += 1,
            Some(false) => same.push(t),
            None => {}
        }
    }
    let total = changed + same.len();
    if total == 0 {
        return None;
    }
    let tail = if same.is_empty() {
        String::new()
    } else {
        format!(", still matches: {}", same.join(", "))
    };
    Some(format!("{changed}/{total} files changed{tail}"))
}

fn clip(s: &str) -> String {
    match s.char_indices().nth(60) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}
