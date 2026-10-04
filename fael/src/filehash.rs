//! Stamp each real file a row names with its blob digest at write time
//! (PLAN-fael-file-hash chunk 1). Core hashes the stream (`core::blob_id_stream`);
//! reading the disk is this side's job — the same side that reads the
//! worktree for `write::check`. `claim` must not call this: a claim is not a
//! check, so it passes the row's old map through instead.

use crate::core;
use crate::hook::is_anchor;
use serde_json::{Map, Value};
use std::path::Path;

/// At most this many files per row, in `files` order (format.md §Rows).
const MAX_FILES: usize = 8;
/// Files larger than this are not hashed — the stream keeps memory flat, but
/// two passes over a huge file would still slow the write.
const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// A `path → 12-hex blob id` map for every real file in `files`, in order.
/// Anchors, globs (no such file on disk), directories, missing files and
/// files over 16 MiB are left out — no key, not an empty value. Empty when
/// nothing qualifies.
pub(crate) fn stamp(root: &Path, files: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    for f in files {
        if out.len() >= MAX_FILES {
            break;
        }
        // a glob is no file on disk, so `blob_at` skips it; a real path that
        // happens to hold `[` (`app/[id]/page.tsx`) is hashed like any other
        if is_anchor(f) {
            continue;
        }
        if let Some(id) = blob_at(root, f) {
            out.insert(f.clone(), Value::String(id));
        }
    }
    out
}

/// What a bump of `id` carries: a bump that moves the row (`--to`, `--urgent`,
/// `--revisit`) is routing, not a check, so it keeps the old map like a claim;
/// a bare `fael bump` is the "still true" check and restamps from disk.
pub(crate) fn for_bump(
    root: &Path,
    log: &core::Log,
    id: &str,
    routing: bool,
) -> Result<Map<String, Value>, String> {
    let old = core::resolve(log, id)?;
    Ok(if routing {
        old.file_hashes().cloned().unwrap_or_default()
    } else {
        stamp(root, &old.files)
    })
}

/// Stamp `row`'s files in place, unless none qualify.
pub(crate) fn stamp_row(root: &Path, row: &mut core::Row) {
    let fh = stamp(root, &row.files);
    if !fh.is_empty() {
        row.extra.insert("fh".into(), Value::Object(fh));
    }
}

/// The 12-hex blob id of one regular file under `root`, or `None` when it is
/// missing, a directory or over `MAX_BYTES`. The stream is capped too, so a
/// file that grows after the size check is still never read whole.
fn blob_at(root: &Path, f: &str) -> Option<String> {
    let p = root.join(f);
    let md = std::fs::metadata(&p).ok()?;
    if !md.is_file() || md.len() > MAX_BYTES {
        return None;
    }
    let mut file = std::fs::File::open(&p).ok()?;
    core::blob_id_stream(&mut file, MAX_BYTES).ok().flatten()
}
