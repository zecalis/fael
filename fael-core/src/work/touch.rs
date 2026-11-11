//! Auto-touch: an edit hook feeds `run.touched` itself, so the overlap line
//! fires with no `fael chunk touch` call. Split out of `db.rs` (file-size
//! ratchet).

use std::path::Path;

/// Add repo-relative edit files to the live runs' `touched`, beside the
/// `last_seen` stamp (overlap-broker f5): an edit hook feeds the overlap
/// line, so `run.touched` fills itself with no `fael chunk touch` call.
/// `docs/` and shared (`.fael/shared`) paths are skipped, as in `overlap.rs`.
/// Fail-open, never migrates: no db, no `touched` column, or nothing to add
/// is 0. Updates the worktree's live runs plus the session's (the
/// `--no-worktree` half, like `mark_seen` + `mark_seen_session`).
pub fn mark_touched(db: &Path, worktree: &str, session: &str, files: &[String]) -> usize {
    if !db.exists() || session.is_empty() || worktree.is_empty() || files.is_empty() {
        return 0;
    }
    let shared = super::pure::shared(Path::new(worktree)).unwrap_or_default();
    let mut good: Vec<String> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for f in files.iter().map(|f| f.trim()).filter(|f| !f.is_empty()) {
        if f.starts_with("docs/") || shared.iter().any(|s| crate::under(f, s)) {
            continue;
        }
        if seen.insert(f.to_string()) {
            good.push(f.to_string());
        }
    }
    if good.is_empty() {
        return 0;
    }
    let Ok(c) = rusqlite::Connection::open(db) else {
        return 0;
    };
    // Merge `good` into one run row's `touched`, newest last, deduped.
    let merge = |cur: Option<String>| {
        let mut seen = std::collections::BTreeSet::new();
        let mut out: Vec<String> = Vec::new();
        for line in cur
            .unwrap_or_default()
            .lines()
            .chain(good.iter().map(String::as_str))
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            if seen.insert(line.to_string()) {
                out.push(line.to_string());
            }
        }
        out.join("\n")
    };
    let mut done = std::collections::BTreeSet::new();
    let mut n = 0usize;
    // Worktree's live runs (the `last_seen` stamp's rows).
    let work_rows: Vec<(i64, Option<String>)> = c
        .prepare(
            "SELECT id, touched FROM run WHERE worktree = ?1 AND ended IS NULL
              AND (session IS NULL OR session = ?2)",
        )
        .and_then(|mut q| {
            q.query_map([worktree, session], |r| Ok((r.get(0)?, r.get(1)?)))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .unwrap_or_default();
    // Missing `touched` column (pre-touch db): fail silent like the fallback.
    if work_rows.is_empty() {
        let has: bool = c.prepare("SELECT touched FROM run LIMIT 0").is_ok();
        if !has {
            return 0;
        }
    }
    for (id, cur) in work_rows {
        let joined = merge(cur);
        if c.execute(
            "UPDATE run SET touched = ?1 WHERE id = ?2",
            rusqlite::params![joined, id],
        )
        .is_ok()
        {
            done.insert(id);
            n += 1;
        }
    }
    // The session's runs wherever they work now (`--no-worktree` half).
    if let Ok(mut q) = c.prepare("SELECT id, touched FROM run WHERE session = ?1 AND ended IS NULL") {
        let rows: Vec<(i64, Option<String>)> = q
            .query_map([session], |r| Ok((r.get(0)?, r.get(1)?)))
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            .unwrap_or_default();
        for (id, cur) in rows {
            if done.contains(&id) {
                continue;
            }
            let joined = merge(cur);
            if c.execute(
                "UPDATE run SET touched = ?1 WHERE id = ?2",
                rusqlite::params![joined, id],
            )
            .is_ok()
            {
                n += 1;
            }
        }
    }
    n
}
