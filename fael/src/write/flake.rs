//! Flake repeats ride the merged write: a flake re-filed on the same key and
//! files carries its occurrence count and date in the new text, so the board
//! shows how often it fired — `×2 2026-10-19`. Mirrors `selfheal::flake` so
//! the count lands exactly when the merge acts.

use crate::core;

/// A flake merge: the new issue and its supersede target share the flake key and
/// the exact file set — mirrors decide.rs so the count rides the merged write.
pub(super) fn is_merge(log: &core::Log, row: &core::Row, target: &str) -> bool {
    let Some(old) = log.rows.iter().find(|x| x.id == target) else {
        return false;
    };
    if !key(row.key.as_deref()) || !key(old.key.as_deref()) || row.key != old.key {
        return false;
    }
    let mut a = row.files.clone();
    let mut b = old.files.clone();
    a.sort_unstable();
    b.sort_unstable();
    !a.is_empty() && a == b
}

fn key(key: Option<&str>) -> bool {
    key.is_some_and(|k| {
        let k = k.to_lowercase();
        k.contains("flake") || k == "e2e:flake" || k.starts_with("e2e:flake:")
    })
}

/// The `×N` count in a flake text — 1 when none is carried yet.
pub(super) fn count(text: &str) -> usize {
    text.split_whitespace()
        .filter_map(|t| {
            t.trim_matches(|c: char| !c.is_alphanumeric() && c != '×')
                .strip_prefix('×')
        })
        .filter_map(|n| n.parse::<usize>().ok())
        .max()
        .unwrap_or(1)
}

/// When the verdict merges a flake, the count line the write stamps.
/// Returns the new text and the info line, or None when no count rides.
pub(super) fn stamped(row: &core::Row, n: usize) -> Option<(String, String)> {
    if count(&row.text) != 1 {
        return None;
    }
    let day = row.ts.get(..10).unwrap_or(&row.ts).to_string();
    let text = format!("{} (flake ×{n} {day})", row.text.trim_end());
    Some((text, format!("flake ×{n} — counted on the open flake row")))
}
