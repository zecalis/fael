//! Rotation for the held rows `chunk done` and `chunk start` name (issue
//! 01M5EEGR): each chunk is a new session, so the per-session shown file cannot
//! tell a row was already put in front of an agent. This machine-local record
//! (row id → last-shown ms, keyed by repo root like `retired/<repo>.txt`) lets
//! the next chunk see the rows the last ones did not. Fail open: unreadable is
//! empty, unwritable drops.

use super::state::{session_key, state_dir};
use crate::core;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn path(root: &Path, dir: &str) -> PathBuf {
    let key = session_key(&root.to_string_lossy());
    state_dir().join(dir).join(format!("{key}.txt"))
}

/// Row id → ms it was last named by a done or start list.
pub(crate) fn load(root: &Path) -> HashMap<String, u64> {
    load_in(root, "held-shown")
}

/// `load` over the record of one list (`start-shown`: what `chunk start` named).
pub(crate) fn load_in(root: &Path, dir: &str) -> HashMap<String, u64> {
    let s = std::fs::read_to_string(path(root, dir)).unwrap_or_default();
    s.lines()
        .filter_map(|l| l.split_once('\t'))
        .filter_map(|(id, ms)| Some((id.to_string(), ms.parse().ok()?)))
        .collect()
}

/// Never-shown rows first in their given (log) order, then least recently shown.
pub(crate) fn order(found: &mut [(&core::Row, String)], seen: &HashMap<String, u64>) {
    found.sort_by_key(|(r, _)| seen.get(&r.id).copied());
}

/// Stamp `ids` shown now. Callers pass the named head only, never the tail.
pub(crate) fn stamp(root: &Path, ids: &[String]) {
    stamp_in(root, "held-shown", ids);
}

/// `stamp` into the record of one list.
pub(crate) fn stamp_in(root: &Path, dir: &str, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let mut seen = load_in(root, dir);
    let now = core::now_ms();
    ids.iter().for_each(|id| {
        seen.insert(id.clone(), now);
    });
    let p = path(root, dir);
    if p.parent()
        .is_none_or(|d| std::fs::create_dir_all(d).is_err())
    {
        return;
    }
    let body: String = seen
        .iter()
        .map(|(id, ms)| format!("{id}\t{ms}\n"))
        .collect();
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(tmp, p);
    }
}
