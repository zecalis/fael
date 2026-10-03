//! Stamp each real file a row names with its git blob id at write time
//! (PLAN-fael-file-hash chunk 1). Core hashes the bytes (`core::blob_id`);
//! reading the disk is this side's job — the same side that reads the
//! worktree for `write::check`. `claim` must not call this: a claim is not a
//! check, so it passes the row's old map through instead.

use crate::core;
use crate::hook::is_anchor;
use serde_json::{Map, Value};
use std::path::Path;

/// At most this many files per row, in `files` order (format.md §Rows).
const MAX_FILES: usize = 8;
/// Files larger than this are not hashed — a row never needs to own a blob
/// that big, and reading it would slow the write.
const MAX_BYTES: u64 = 1024 * 1024;

/// A `path → 12-hex blob id` map for every real file in `files`, in order.
/// Anchors, globs, directories, missing files and files over 1 MiB are left
/// out — no key, not an empty value. Empty when nothing qualifies.
pub(crate) fn stamp(root: &Path, files: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    for f in files {
        if out.len() >= MAX_FILES {
            break;
        }
        if is_anchor(f) || is_glob(f) {
            continue;
        }
        if let Some(id) = blob_at(root, f) {
            out.insert(f.clone(), Value::String(id));
        }
    }
    out
}

/// Stamp `row`'s files in place, unless none qualify.
pub(crate) fn stamp_row(root: &Path, row: &mut core::Row) {
    let fh = stamp(root, &row.files);
    if !fh.is_empty() {
        row.extra.insert("fh".into(), Value::Object(fh));
    }
}

/// The 12-hex blob id of one regular file under `root`, or `None` when it is
/// missing, a directory or over `MAX_BYTES`.
fn blob_at(root: &Path, f: &str) -> Option<String> {
    let p = root.join(f);
    let md = std::fs::metadata(&p).ok()?;
    if !md.is_file() || md.len() > MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(&p).ok()?;
    Some(core::blob_id(&bytes))
}

/// A file glob (`*`, `?`, `[...]`) is a pattern, not a path — `find` matches
/// it the same way, so it always passes the write check.
pub(crate) fn is_glob(f: &str) -> bool {
    f.contains(['*', '?', '['])
}
