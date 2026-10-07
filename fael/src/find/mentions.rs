//! Who cites a row: `find <id>` names the rows that mention it, so the reader
//! sees who cites it without knowing a flag for it. Split from `find.rs` at
//! the 400-line ratchet; no callers change shape.

use fael_core::{self as core, Log, Row};

/// Short ids of rows that cite `row`: its full id, its rendered short, or any
/// shorter prefix that short decayed from as the log grew — an 8+ char token
/// of the id's own prefix, never a longer id that merely shares the prefix.
/// Self-hits drop out. Capped at 5, like `mentioned`.
pub(crate) fn mentioners(log: &Log, row: &Row) -> Vec<String> {
    let ab = core::abbrev(log);
    let id = row.id.to_lowercase();
    let cites = |r: &Row| {
        [Some(r.text.as_str()), r.title.as_deref()]
            .into_iter()
            .flatten()
            .any(|s| {
                let s = s.to_lowercase();
                s.contains(&id)
                    || s.split(|c: char| !c.is_ascii_alphanumeric())
                        .filter(|w| w.len() >= 8)
                        .any(|w| id.starts_with(w))
            })
    };
    let mut who: Vec<String> = log
        .rows
        .iter()
        .chain(log.closes.iter())
        .filter(|r| cites(r))
        .map(|r| ab.short(&r.id).to_string())
        .take(5)
        .collect();
    who.sort();
    who.dedup();
    who.into_iter()
        .filter(|s| !row.id.starts_with(s.as_str()))
        .take(5)
        .collect()
}

/// Short ids of rows whose text or title merely mentions `tok` (open rows,
/// then close reasons), capped at 5. Case-insensitive — ids match that way too.
pub(crate) fn mentioned(log: &Log, tok: &str) -> Vec<String> {
    let ab = core::abbrev(log);
    let needle = tok.to_lowercase();
    let names = |s: Option<&str>| s.is_some_and(|s| s.to_lowercase().contains(&needle));
    log.rows
        .iter()
        .chain(log.closes.iter())
        .filter(|r| names(Some(&r.text)) || names(r.title.as_deref()))
        .map(|r| ab.short(&r.id).to_string())
        .take(5)
        .collect()
}
