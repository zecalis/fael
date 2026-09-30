//! Local tombstones: the ids `fael purge` removed, in `<journal>/purged.txt`.
//! A purge cannot delete a row from a remote (pushes are ff-only), so sync
//! carries the id instead: it goes into this writer's ref as `purged.txt`, and
//! every reader's ingest and push skip the row (docs/sync-format.md).

use crate::Repo;
use std::collections::BTreeSet;
use std::io::Write;

/// Remember `id` as purged. No journal (no readable `.git`) means no sync, so
/// nothing to carry and nothing to write.
pub(crate) fn record(r: &Repo, id: &str) -> Result<(), String> {
    let Some(dir) = r.journal.as_deref() else {
        return Ok(());
    };
    let path = dir.join(fael_core::sync::PURGED);
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(dir).map_err(io)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(io)?;
    writeln!(f, "{id}").map_err(io)
}

/// Every id purged in this clone.
pub(super) fn local(r: &Repo) -> BTreeSet<String> {
    let body = r
        .journal
        .as_deref()
        .and_then(|d| std::fs::read_to_string(d.join(fael_core::sync::PURGED)).ok());
    body.map(|b| fael_core::sync::parse_purged(&b).into_iter().collect())
        .unwrap_or_default()
}
