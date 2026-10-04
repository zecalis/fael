//! `fael purge` — permanently remove one row from the log files.
//!
//! The escape hatch for test rows and mistakes that leaked into a shared log:
//! `restore` only reverts supersede edges, it cannot delete. Purge is the one
//! deliberate exception to the append-only log — it rewrites month files, so
//! it refuses whenever anything still points at the row:
//!
//! - another row's `supersedes`/`restores` names it → purge that row first
//! - the id names a close event, not a row → `fael restore` reopens those
//! - any copy lives in an immutable file (`compact.*`, `_import/*`)
//! - a file to rewrite has lines `read` would skip → `fael doctor --fix` first
//!
//! The row's own close and bump events go with it (a close without its row
//! is a `[Phantom]` `doctor` would flag; a bump event would keep the id
//! syncing after the tombstone). Both stores are rewritten — the tree and
//! the journal when the clone has one — following `compact` (locks, tmp +
//! rename, same bytes to every root that held a source line). Copies already
//! synced to other clones or remotes are out of reach; the CLI keeps the id as
//! a tombstone so `fael sync` never carries the row back, and warns about the
//! copies in teammates' journals when `fael.remote` is set.

use super::{Log, collect_files, lock, month_of, tmp_rename};
use crate::{Row, decode_text, resolve};
use std::path::{Path, PathBuf};

/// What `purge_row` removed: the row's add lines and bump events (`rows`),
/// its close events (`closes`), and every rewritten file (absolute paths).
#[derive(Debug, Default, PartialEq)]
pub struct Purged {
    pub id: String,
    pub title: String,
    pub rows: usize,
    pub closes: usize,
    pub files: Vec<PathBuf>,
}

/// One file's surviving lines plus what was dropped — `None` when the file
/// holds nothing of the row and stays untouched.
struct Scanned {
    kept: Vec<String>,
    rows: usize,
    closes: usize,
}

/// Remove one row (exact id or unique prefix, like everywhere else) from every
/// month file in both stores. See the module docs for the refusal rules.
pub fn purge_row(
    fael: &Path,
    journal: Option<&Path>,
    log: &Log,
    id: &str,
) -> Result<Purged, String> {
    let target = resolve_target(log, id)?;
    refuse_blockers(log, &target.id)?;
    // tree first, like `compact` — one lock per store across the rewrite
    let stores: Vec<&Path> = match journal {
        Some(j) if j != fael => vec![fael, j],
        _ => vec![fael],
    };
    // a store without `log/` has nothing to rewrite — and locking it would create
    // it, which fails (EEXIST) on a worktree whose `.fael` symlinks to a missing dir
    let stores: Vec<&Path> = stores
        .into_iter()
        .filter(|s| s.join("log").is_dir())
        .collect();
    let _guards = stores
        .iter()
        .map(|s| lock(s))
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Purged {
        id: target.id.clone(),
        title: target.display_title(),
        ..Purged::default()
    };
    for s in &stores {
        for f in collect_files(&s.join("log")) {
            if !f.to_string_lossy().ends_with(".jsonl") {
                continue;
            }
            let Some(sc) = scan_file(&f, &target.id)? else {
                continue;
            };
            let mut body = sc.kept.join("\n");
            if !sc.kept.is_empty() {
                body.push('\n');
            }
            tmp_rename(&f, body.as_bytes())?;
            out.rows += sc.rows;
            out.closes += sc.closes;
            out.files.push(f);
        }
    }
    Ok(out)
}

/// The target row — or a pointer at `fael restore` when the id names a close
/// event, which purge never removes on its own.
fn resolve_target(log: &Log, id: &str) -> Result<Row, String> {
    match resolve(log, id) {
        Ok(r) => Ok(r.clone()),
        Err(e) => {
            if log.closes.iter().any(|c| c.id == id) {
                let back = log
                    .closes
                    .iter()
                    .find(|c| c.id == id)
                    .and_then(|c| c.reference.clone())
                    .unwrap_or_default();
                return Err(format!(
                    "rejected: {id} is a close event for {back} — reopen it with `fael restore`, purge removes whole rows"
                ));
            }
            Err(e)
        }
    }
}

/// Refuse when another row's edge still names the target — purging under a
/// live `supersedes`/`restores` would leave a dangling pointer.
fn refuse_blockers(log: &Log, tid: &str) -> Result<(), String> {
    let mut blockers = vec![];
    for r in &log.rows {
        if r.id == tid {
            continue;
        }
        if r.supersedes.as_deref() == Some(tid) {
            blockers.push(format!("{} supersedes", r.id));
        }
        if r.restores.as_deref() == Some(tid) {
            blockers.push(format!("{} restores", r.id));
        }
    }
    if blockers.is_empty() {
        return Ok(());
    }
    Err(format!(
        "rejected: {} still point at {tid} — purge them first",
        blockers.join(", ")
    ))
}

/// Split one file into surviving lines and drop counts. Refuses immutable
/// files and files with lines `read` would skip — purge never drops a byte
/// silently. Add lines match by `id` in any file, bump events by `bumps`;
/// close events match by `ref` in `.close.jsonl` files only (a stray `ref`
/// on an add row is a citation, not lifecycle).
fn scan_file(path: &Path, tid: &str) -> Result<Option<Scanned>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if String::from_utf8(bytes.clone()).is_err() {
        return Err(format!(
            "{}: not UTF-8 — run `fael doctor --fix` first, purge never drops a byte",
            path.display()
        ));
    }
    let text = decode_text(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    let tail = lines.pop().unwrap_or("");
    if !tail.trim().is_empty() {
        return Err(format!(
            "{}: torn last line (no \\n) — run `fael doctor --fix` first, purge never drops a byte",
            path.display()
        ));
    }
    let close_file = path.to_string_lossy().ends_with(".close.jsonl");
    let mut sc = Scanned {
        kept: vec![],
        rows: 0,
        closes: 0,
    };
    for line in lines {
        let t = line.trim();
        if t.is_empty() || super::is_marker(t) {
            sc.kept.push(line.to_string());
            continue;
        }
        let row: Row = serde_json::from_str(t).map_err(|e| {
            format!(
                "{}: broken line ({e}) — run `fael doctor --fix` first, purge never drops a byte",
                path.display()
            )
        })?;
        if row.id == tid || row.bumps.as_deref() == Some(tid) {
            if close_file {
                sc.closes += 1;
            } else {
                sc.rows += 1;
            }
        } else if close_file && row.reference.as_deref() == Some(tid) {
            sc.closes += 1;
        } else {
            sc.kept.push(line.to_string());
        }
    }
    if sc.rows + sc.closes == 0 {
        return Ok(None);
    }
    if month_of(path).is_none() {
        return Err(format!(
            "rejected: {tid} lives in immutable {} — purge only touches <writer>/<yyyy-mm>[.close].jsonl",
            path.display()
        ));
    }
    Ok(Some(sc))
}
