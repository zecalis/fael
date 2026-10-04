//! Which open rows' files changed since the row was written
//! (PLAN-fael-file-hash chunk 2). The row's `fh` map holds each real file's
//! git blob id at write time; this module compares those against the bytes on
//! disk now, resolved through L1 aliases (a rename reads the new path).
//! Pure disk reads, no git spawn — safe on the 5 ms push path. A row with no
//! `fh`, or a file that resolves nowhere, is unknown, never changed.

use crate::core;
// files bigger than this are never stamped; a stamped file that grew past it
// changed — no read needed to say so
use crate::filehash::MAX_BYTES as STAMP_MAX;
use std::collections::HashMap;
use std::path::Path;

/// The push path hashes at most this much per file: it has 5 ms and no git.
/// A file between this and `STAMP_MAX` is unknown here — hashing 16 MiB would
/// take ~20 ms — and the generic hint stands in; doctor, off the hot path, can
/// still compare it.
const PUSH_MAX: u64 = 1024 * 1024;

/// Said under the rows of an edit push (see `push`).
/// The usage event `fael-core::stats` reads as "in context at edit".
const STALE_HINT: &str = "fael: a row above the code now says or contradicts? `fael close <id> \"now in <file>\"` or re-file it with `--supersedes <id>`";

/// The row's verdict against the worktree: `Some(true)` when any stamped file
/// differs, `Some(false)` when every stamped file matches, `None` when the
/// row carries no `fh` or a file resolves nowhere (gone, renamed in a cycle,
/// unreadable — `[Gone]` owns the gone case, never "changed").
///
/// Chunk-2 wires this with no direct caller — `stale_hint` below shares one
/// blob cache across the push's rows instead; chunk-3 (shadow `changed:`
/// usage) calls this per shown row. The `allow(dead_code)` goes away then.
#[allow(dead_code)]
pub(crate) fn changed(row: &core::Row, root: &Path, al: &core::Aliases) -> Option<bool> {
    changed_cached(row, root, al, &mut HashMap::new())
}

/// `changed` with a shared blob cache, so one push reads each file once
/// however many rows name it (hub files carry dozens of rows).
fn changed_cached(
    row: &core::Row,
    root: &Path,
    al: &core::Aliases,
    blobs: &mut HashMap<String, Option<String>>,
) -> Option<bool> {
    let fh = row.file_hashes()?;
    if fh.is_empty() {
        return None;
    }
    let mut unknown = false;
    for (f, v) in fh {
        let (Some(want), Some(target)) = (v.as_str(), resolve(al, f)) else {
            unknown = true;
            continue;
        };
        match file_verdict(want, &target, root, blobs) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => unknown = true,
        }
    }
    if unknown { None } else { Some(false) }
}

/// Where `f` lives now: through L1 renames, else itself. `None` when the
/// renames cycle, so there is no single answer — unknown, never changed.
fn resolve(al: &core::Aliases, f: &str) -> Option<String> {
    match al.current(f) {
        Some(t) => Some(t),
        None if al.forward(f).len() > 1 => None,
        None => Some(f.to_string()),
    }
}

/// One stamped file against disk: `None` when it is gone, a directory or
/// unreadable; `Some(true)` without reading when it grew past the stamp cap.
fn file_verdict(
    want: &str,
    target: &str,
    root: &Path,
    blobs: &mut HashMap<String, Option<String>>,
) -> Option<bool> {
    let blob = blobs
        .entry(target.to_string())
        .or_insert_with(|| blob_at(root, target));
    blob.as_ref().map(|now| now != want)
}

/// The 12-hex blob id on disk, or `None` when there is nothing hashable here
/// (gone, a directory, or over `PUSH_MAX`). `Some("")` marks a file that grew
/// past the stamp cap — anything compares unequal to it, so it reads as
/// changed without the read.
fn blob_at(root: &Path, target: &str) -> Option<String> {
    let p = root.join(target);
    let md = std::fs::metadata(&p).ok()?;
    if !md.is_file() {
        return None;
    }
    if md.len() > STAMP_MAX {
        return Some(String::new());
    }
    if md.len() > PUSH_MAX {
        return None;
    }
    core::blob_id_stream(&mut std::fs::File::open(&p).ok()?, PUSH_MAX)
        .ok()
        .flatten()
}

/// The edit hint: tier-0 rows whose files changed since the row was written
/// are named (at most two) with the retire ready to run; rows whose files all
/// match earn no hint; rows with no verdict keep the legacy hint (named open
/// issues, else the generic clause). `None` when no hint is earned.
pub(crate) fn stale_hint(
    log: &core::Log,
    tiered: &[(&core::Row, usize)],
    files: &[String],
    root: &Path,
    al: &core::Aliases,
) -> Option<String> {
    let mut blobs = HashMap::new();
    let mut changed_rows: Vec<&core::Row> = vec![];
    let mut unknown_rows: Vec<&core::Row> = vec![];
    for (r, tier) in tiered {
        if *tier != 0 {
            continue;
        }
        match changed_cached(r, root, al, &mut blobs) {
            Some(true) => changed_rows.push(r),
            Some(false) => {}
            None => unknown_rows.push(r),
        }
    }
    if !changed_rows.is_empty() {
        changed_rows.truncate(2);
        return Some(changed_hint(log, &changed_rows, files));
    }
    if unknown_rows.is_empty() {
        return None;
    }
    Some(legacy_hint(log, &unknown_rows))
}

/// `src/a.rs changed since <id> was written` — one line per row, each with
/// the bump, the supersede re-file and the close ready to run.
fn changed_hint(log: &core::Log, rows: &[&core::Row], files: &[String]) -> String {
    let ab = core::abbrev(log);
    let file = files.join(", ");
    rows.iter()
        .map(|r| {
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
