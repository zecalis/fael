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

/// Why a real file got no key in the map.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Why {
    Oversize,
    Unreadable,
    PastCap,
}

/// A real file `stamp` left out, and why.
pub(crate) type Skipped = (String, Why);

/// A `path → 12-hex blob id` map for every real file in `files`, in order,
/// plus the real files left out and why (over 16 MiB, unreadable, past the
/// 8th). Anchors, globs (no such file on disk), directories and missing files
/// are left out silently — no key, not an empty value: they are not the
/// author's gap. The map is empty when nothing qualifies.
pub(crate) fn stamp(root: &Path, files: &[String]) -> (Map<String, Value>, Vec<Skipped>) {
    let mut out = Map::new();
    let mut skipped = vec![];
    for f in files {
        // a glob is no file on disk, so `blob_at` skips it; a real path that
        // happens to hold `[` (`app/[id]/page.tsx`) is hashed like any other
        if is_anchor(f) {
            continue;
        }
        if out.len() >= MAX_FILES {
            if root.join(f).is_file() {
                skipped.push((f.clone(), Why::PastCap));
            }
            continue;
        }
        match blob_at(root, f) {
            Some(Ok(id)) => {
                out.insert(f.clone(), Value::String(id));
            }
            Some(Err(why)) => skipped.push((f.clone(), why)),
            None => {}
        }
    }
    (out, skipped)
}

/// The one info line for what `stamp` left out, or `None` when nothing was.
/// No `warning:` prefix on purpose — like the phantom-id line it is said to
/// the author, never counted, and never fails the write.
pub(crate) fn skipped_note(skipped: &[Skipped]) -> Option<String> {
    const SHOWN: usize = 5;
    if skipped.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = skipped
        .iter()
        .take(SHOWN)
        .map(|(f, why)| {
            let why = match why {
                Why::Oversize => format!("over {} MiB", MAX_BYTES >> 20),
                Why::Unreadable => "unreadable".to_string(),
                Why::PastCap => format!("past the {MAX_FILES}-file cap"),
            };
            format!("{f} ({why})")
        })
        .collect();
    if skipped.len() > SHOWN {
        parts.push(format!("+{} more", skipped.len() - SHOWN));
    }
    Some(format!(
        "fael: not stamped (no file-hash verdict at push): {}",
        parts.join(", ")
    ))
}

/// What a bump of `id` carries, and the note for what it left out: a bump
/// that moves the row (`--to`, `--urgent`, `--revisit`) is routing, not a
/// check, so it keeps the old map like a claim (no restamp, no note); a bare
/// `fael bump` is the "still true" check and restamps from disk.
pub(crate) fn for_bump(
    root: &Path,
    log: &core::Log,
    id: &str,
    routing: bool,
) -> Result<(Map<String, Value>, Option<String>), String> {
    let old = core::resolve_row(log, id)?;
    Ok(if routing {
        (old.file_hashes().cloned().unwrap_or_default(), None)
    } else {
        let (fh, skipped) = stamp(root, &old.files);
        (fh, skipped_note(&skipped))
    })
}

/// Stamp `row`'s files in place, unless none qualify. Returns the note for
/// any real file left out.
pub(crate) fn stamp_row(root: &Path, row: &mut core::Row) -> Option<String> {
    let (fh, skipped) = stamp(root, &row.files);
    if !fh.is_empty() {
        row.extra.insert("fh".into(), Value::Object(fh));
    }
    skipped_note(&skipped)
}

/// The 12-hex blob id of one regular file under `root`: `None` when it is
/// missing or a directory (not the author's gap), `Err` when it is a real
/// file that cannot be stamped. The stream is capped too, so a file that
/// grows after the size check is still never read whole.
fn blob_at(root: &Path, f: &str) -> Option<Result<String, Why>> {
    let p = root.join(f);
    let md = std::fs::metadata(&p).ok()?;
    if !md.is_file() {
        return None;
    }
    if md.len() > MAX_BYTES {
        return Some(Err(Why::Oversize));
    }
    let Ok(mut file) = std::fs::File::open(&p) else {
        return Some(Err(Why::Unreadable));
    };
    Some(match core::blob_id_stream(&mut file, MAX_BYTES) {
        Ok(Some(id)) => Ok(id),
        // grew or shrank under the read — no stable bytes to name
        Ok(None) | Err(_) => Err(Why::Unreadable),
    })
}
