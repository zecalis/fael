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

/// Occurrence lines kept on a merged flake row.
// ponytail: newest 3 occurrences stay, older ones drop; the first report and
// the `×N` count (max over lines) survive. Raise if reports need more history.
const KEEP: usize = 3;

/// When the verdict merges a flake, the merged text: the prior text kept, plus
/// a `×N <day>: <new text>` line. Returns it and the info line, or None when
/// the new text already carries a count.
pub(super) fn stamped(row: &core::Row, old: &str, n: usize) -> Option<(String, String)> {
    if count(&row.text) != 1 {
        return None;
    }
    let day = row.ts.get(..10).unwrap_or(&row.ts);
    let new = row.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let (mut occ, base): (Vec<&str>, Vec<&str>) =
        old.trim_end().lines().partition(|l| l.starts_with('×'));
    let line = format!("×{n} {day}: {new}");
    occ.push(&line);
    let skip = occ.len().saturating_sub(KEEP);
    let text = base
        .into_iter()
        .chain(occ.into_iter().skip(skip))
        .collect::<Vec<_>>()
        .join("\n");
    Some((text, format!("flake ×{n} — counted on the open flake row")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(text: &str) -> core::Row {
        core::Row {
            text: text.into(),
            ts: "2026-10-20T01:02:03Z".into(),
            ..Default::default()
        }
    }

    #[test]
    fn merge_keeps_prior_text_and_counts() {
        let (t, _) = stamped(&row("second"), "first", 2).unwrap();
        assert_eq!(t, "first\n×2 2026-10-20: second");
        assert_eq!(count(&t), 2);
    }

    #[test]
    fn merge_is_bounded_and_count_survives() {
        let mut t = "first".to_string();
        for n in 2..=8 {
            t = stamped(&row(&format!("r{n}")), &t, n).unwrap().0;
        }
        assert_eq!(t.lines().count(), 1 + KEEP);
        assert!(t.starts_with("first\n"));
        assert!(t.contains("×6 ") && t.ends_with("r8"));
        assert!(!t.contains("r5"));
        assert_eq!(count(&t), 8);
    }
}
