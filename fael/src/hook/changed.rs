//! Which open rows' files changed since the row was written
//! (PLAN-fael-file-hash chunk 2). The row's `fh` map holds each real file's
//! git blob id at write time; this module compares those against the bytes on
//! disk now, resolved through L1 aliases (a rename reads the new path).
//! Pure disk reads, no git spawn — safe on the 5 ms push path. A row with no
//! `fh`, or a file that resolves nowhere, is unknown, never changed.

use crate::core;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Files bigger than this are never hashed on the push path (the stamp side
/// covers up to 16 MiB — 01M42CGE). A file over this cap has no verdict here,
/// even when stamped: hashing it would break the 5 ms push ceiling.
const MAX_BYTES: u64 = 1024 * 1024;

/// The generic clause of the legacy hint: said for rows with no verdict.
const STALE_HINT: &str = "fael: a row above the code now says or contradicts? `fael close <id> \"now in <file>\"` or re-file it with `--supersedes <id>`";

/// One blob cache per push, so each file is read once however many rows name
/// it (hub files carry dozens of rows) and the edit hint and the shadow split
/// share the reads.
pub(crate) type Blobs = HashMap<String, Option<String>>;

/// The shadow split: full ids of the `changed` rows and of the `unchanged` rows.
pub(crate) type Split = (Vec<String>, Vec<String>);

/// A row against the worktree. `Changed` carries the first stamped file (as it
/// lives now) that differs; `Unknown` is a row with no `fh` or a file that
/// resolves nowhere (gone, renamed in a cycle, unreadable or over the push cap
/// — `[Gone]` owns the gone case, never "changed").
enum Verdict {
    Changed(String),
    Same,
    Unknown,
}

/// The shadow split for a push's usage line (PLAN-fael-file-hash chunk 3):
/// full ids of the shown rows whose files changed since the row was written,
/// and full ids of the shown rows whose files all still match. Rows with no
/// verdict ride in neither list — never guessed as changed. Nothing renders:
/// the caller records the two lists on the usage line only.
fn partition(rows: &[&core::Row], root: &Path, al: &core::Aliases, blobs: &mut Blobs) -> Split {
    let mut changed_ids = vec![];
    let mut unchanged_ids = vec![];
    for r in rows {
        match verdict(r, root, al, blobs) {
            Verdict::Changed(_) => changed_ids.push(r.id.clone()),
            Verdict::Same => unchanged_ids.push(r.id.clone()),
            Verdict::Unknown => {}
        }
    }
    (changed_ids, unchanged_ids)
}

fn verdict(row: &core::Row, root: &Path, al: &core::Aliases, blobs: &mut Blobs) -> Verdict {
    let Some(fh) = row.file_hashes().filter(|fh| !fh.is_empty()) else {
        return Verdict::Unknown;
    };
    let mut unknown = false;
    for (f, v) in fh {
        let (Some(want), Some(target)) = (v.as_str(), resolve(al, root, f)) else {
            unknown = true;
            continue;
        };
        match file_verdict(want, &target, root, blobs) {
            Some(true) => return Verdict::Changed(target),
            Some(false) => {}
            None => unknown = true,
        }
    }
    if unknown {
        Verdict::Unknown
    } else {
        Verdict::Same
    }
}

/// Where `f` lives now: through L1 renames, else itself — a path that exists
/// again after being renamed away (a split) is read as itself. `None` when the
/// renames cycle, so there is no single answer — unknown, never changed.
fn resolve(al: &core::Aliases, root: &Path, f: &str) -> Option<String> {
    match al.current(f) {
        Some(t) if !root.join(f).is_file() => Some(t),
        Some(_) => Some(f.to_string()),
        None if al.forward(f).len() > 1 => None,
        None => Some(f.to_string()),
    }
}

/// One stamped file against disk: `None` when it is gone, a directory,
/// unreadable, or over the push cap below — never guessed as changed.
fn file_verdict(want: &str, target: &str, root: &Path, blobs: &mut Blobs) -> Option<bool> {
    let blob = blobs
        .entry(target.to_string())
        .or_insert_with(|| blob_at(root, target));
    blob.as_ref().map(|now| now != want)
}

/// The 12-hex blob id on disk, or `None` when there is nothing hashable.
/// Files over `MAX_BYTES` have no verdict: the stamp side covers up to 16
/// MiB, but hashing that much on the push path would break its 5 ms ceiling
/// (01M42CGE) — unknown keeps the legacy hint, never a false "changed".
fn blob_at(root: &Path, target: &str) -> Option<String> {
    let md = std::fs::metadata(root.join(target)).ok()?;
    if !md.is_file() {
        return None;
    }
    if md.len() > MAX_BYTES {
        return None;
    }
    let mut file = std::fs::File::open(root.join(target)).ok()?;
    core::blob_id_stream(&mut file, MAX_BYTES).ok().flatten()
}

/// The said rows' ids plus, on a read, their shadow split (PLAN-fael-file-hash
/// chunk 3): what fit the budget, of which `changed` files moved since the row
/// was written and `unchanged` still match — rows with no verdict ride
/// neither. An edit gets no split: the file on disk already holds the edit.
pub(crate) fn split_said(
    sel: &core::Selection<'_>,
    n: usize,
    edit: bool,
    root: &Path,
    al: &core::Aliases,
    blobs: &mut Blobs,
) -> (Vec<String>, Option<Split>) {
    let said: Vec<&core::Row> = sel.shown.iter().take(n).copied().collect();
    let shown: Vec<String> = said.iter().map(|r| r.id.clone()).collect();
    (shown, (!edit).then(|| partition(&said, root, al, blobs)))
}

/// The tier-0 rows of an edit push the agent has in front of it — said now
/// (`said`) or by an earlier push this session (`told`) — through `stale_hint`.
/// A row cut by the cap or the budget was never said, so it is never named.
pub(crate) fn edit_hint(
    log: &core::Log,
    t0: &[(&core::Row, usize)],
    told: &HashSet<String>,
    said: &[&core::Row],
    root: &Path,
    al: &core::Aliases,
    blobs: &mut Blobs,
) -> Option<String> {
    let rows: Vec<&core::Row> = t0
        .iter()
        .filter(|(r, tier)| {
            *tier == 0 && (told.contains(&r.id) || said.iter().any(|s| s.id == r.id))
        })
        .map(|(r, _)| *r)
        .collect();
    stale_hint(log, &rows, root, al, blobs)
}

/// The edit hint over the tier-0 rows already in the agent's context: rows
/// whose files changed since the row was written are named (at most two, each
/// with the changed file and the retire ready to run); rows whose files all
/// match earn no hint; rows with no verdict keep the legacy hint (named open
/// issues, else the generic clause) — beside the named rows only for open
/// issues. `None` when no hint is earned.
///
/// The edit hook runs after the write (PostToolUse), so "changed since the row
/// was written" includes the edit just made: a row filed before it is the one
/// to re-check.
fn stale_hint(
    log: &core::Log,
    rows: &[&core::Row],
    root: &Path,
    al: &core::Aliases,
    blobs: &mut Blobs,
) -> Option<String> {
    let mut changed_rows: Vec<(&core::Row, String)> = vec![];
    let mut unknown_rows: Vec<&core::Row> = vec![];
    for r in rows {
        match verdict(r, root, al, blobs) {
            Verdict::Changed(f) => changed_rows.push((r, f)),
            Verdict::Same => {}
            Verdict::Unknown => unknown_rows.push(r),
        }
    }
    changed_rows.truncate(2);
    if !changed_rows.is_empty() {
        unknown_rows.retain(|r| r.kind == "issue");
    }
    let mut lines = vec![];
    if !changed_rows.is_empty() {
        lines.push(changed_hint(log, &changed_rows));
    }
    if !unknown_rows.is_empty() {
        lines.push(legacy_hint(log, &unknown_rows));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// `src/a.rs changed since <id> was written` — one line per row, each with
/// the bump, the supersede re-file and the close ready to run.
fn changed_hint(log: &core::Log, rows: &[(&core::Row, String)]) -> String {
    let ab = core::abbrev(log);
    rows.iter()
        .map(|(r, file)| {
            let s = ab.short(&r.id);
            format!(
                "fael: {file} changed since {s} was written — still true? `fael bump {s}` · wrong now? re-file with `--supersedes {s}` · done? `fael close {s} \"now in <file>\"`"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The pre-hash hint, kept for rows with no verdict: open issues in context
/// get the ready close (at most two), anything else the generic clause.
fn legacy_hint(log: &core::Log, rows: &[&core::Row]) -> String {
    let issues: Vec<&&core::Row> = rows.iter().filter(|r| r.kind == "issue").take(2).collect();
    if issues.is_empty() {
        return STALE_HINT.to_string();
    }
    let ab = core::abbrev(log);
    let calls: Vec<String> = issues
        .iter()
        .map(|r| format!("fael close {} \"<why>\"", ab.short(&r.id)))
        .collect();
    format!(
        "fael: done with one? {} — any other row the code now says or contradicts: `fael close <id> \"now in <file>\"` or re-file it with `--supersedes <id>`",
        calls.join(" · ")
    )
}
