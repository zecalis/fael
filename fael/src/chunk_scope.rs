//! `chunk add|edit --scope`: each path checked against the plan's checkout once written
//! (issue 01M4Q4HHX) — a typo'd scope silently weakens every overlap check, so it is said;
//! never refused, a chunk may create the file.

use fael_core::plan::Store;
use std::path::{Path, PathBuf};

/// A warning on stderr for each scope path of chunk `key` that names nothing in the plan's
/// checkout (`fael plan repo`, else `root`): its directory is missing, or holds a name within
/// edit distance 2. A new file or directory under an existing one passes silently.
pub(crate) fn warn(s: &Store, root: &Path, key: &str) {
    let Ok(b) = s.show(key) else { return };
    let at = b.repo.map_or_else(|| root.to_path_buf(), PathBuf::from);
    for p in b.scope.as_deref().unwrap_or_default().lines() {
        if let Some(why) = missing(&at, p) {
            eprintln!(
                "warning: scope {p:?} {why} in {} — a typo? fael chunk edit {key} --scope …",
                at.display()
            );
        }
    }
}

fn missing(at: &Path, p: &str) -> Option<String> {
    let p = p.trim_end_matches('/');
    if p.is_empty() || at.join(p).exists() {
        return None;
    }
    let dir = p.rsplit_once('/').map_or("", |(d, _)| d);
    if !at.join(dir).is_dir() {
        return Some(format!("names nothing: no directory {dir:?}"));
    }
    crate::write::sibling_suggest(at, p).map(|near| format!("is not on disk — {near:?}?"))
}
