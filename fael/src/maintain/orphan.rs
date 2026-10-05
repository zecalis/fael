//! `[Orphan]` (PLAN-fael-row-hygiene chunk 6): open rows filed on a branch
//! whose PRs all closed unmerged. Split out of `maintain.rs` for the 400-line
//! cap, and because the `gh` half must never live in core (core spawns no
//! processes).

use crate::core;
use std::collections::HashSet;

/// `branch → full row id(s)` for every branch on open rows whose PRs all
/// closed unmerged — a live open PR off the branch is not orphan (review
/// finding). One `gh` call for the whole repo, not one per branch; no `gh`, no
/// auth, or no PR for the branch → skipped silently, never an error. Full ids
/// (not the abbreviated examples) ride to `doctor --json` so a cleanup agent
/// can act on them.
pub(super) fn rows(log: &core::Log) -> Vec<(String, Vec<String>)> {
    let mut by_branch: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for row in core::find(log, &core::Filter::default()) {
        if let Some(b) = row.branch().filter(|b| !b.is_empty()) {
            by_branch
                .entry(b.to_string())
                .or_default()
                .push(row.id.clone());
        }
    }
    if by_branch.is_empty() {
        return vec![];
    }
    let Some(dead) = dead_branches() else {
        return vec![];
    };
    by_branch
        .into_iter()
        .filter(|(b, _)| dead.contains(b))
        .collect()
}

/// Branches whose PRs all closed unmerged; None = unknown (no `gh`, or it
/// failed). `--state all` so an open PR off a branch is seen — a branch with
/// a fresh open PR is not orphan just because an earlier PR closed unmerged.
///
/// `FAEL_GH_JSON` short-circuits the spawn with canned output (tests only —
/// Windows `CreateProcess` never resolves a `.bat` fake off PATH, and CI
/// runners ship a real `gh` that answers on its own, so no fake survives
/// there; cf. `FAEL_STATE_DIR`).
// ponytail: newest 200 PRs; a branch whose PRs all fell out of the window is
// unknown (silent), not orphan — raise --limit if a repo outgrows it.
fn dead_branches() -> Option<HashSet<String>> {
    if let Ok(fake) = std::env::var("FAEL_GH_JSON") {
        return parse_dead(&fake);
    }
    let out = std::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--json",
            "headRefName,state,mergedAt",
            "--limit",
            "200",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_dead(&String::from_utf8_lossy(&out.stdout))
}

/// Pure half of the above: None on unparseable output. Entries without a
/// head name are skipped. Per branch: an `OPEN` PR means it is still alive;
/// otherwise it is dead when every PR is unmerged — a missing/null `mergedAt`
/// is unmerged, and so is gh's zero time `0001-…`.
fn parse_dead(json: &str) -> Option<HashSet<String>> {
    let ps = serde_json::from_str::<serde_json::Value>(json).ok()?;
    let mut by_head: std::collections::BTreeMap<&str, Vec<&serde_json::Value>> =
        std::collections::BTreeMap::new();
    for p in ps.as_array()? {
        if let Some(h) = p.get("headRefName").and_then(|h| h.as_str()) {
            by_head.entry(h).or_default().push(p);
        }
    }
    Some(
        by_head
            .into_iter()
            .filter(|(_, ps)| {
                ps.iter()
                    .all(|p| p.get("state").and_then(|s| s.as_str()) != Some("OPEN"))
                    && ps.iter().all(|p| match p.get("mergedAt") {
                        None | Some(serde_json::Value::Null) => true,
                        Some(serde_json::Value::String(s)) => s.starts_with("0001-"),
                        _ => false,
                    })
            })
            .map(|(h, _)| h.to_string())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_dead;

    fn dead(json: &str) -> Vec<String> {
        let mut v: Vec<String> = parse_dead(json).unwrap().into_iter().collect();
        v.sort();
        v
    }

    #[test]
    fn orphan_parses_gh_state_and_merged_at() {
        assert_eq!(parse_dead("not json"), None);
        assert_eq!(parse_dead("{}"), None);
        assert!(dead("[]").is_empty()); // no PR: nothing to say
        assert!(dead(r#"[{"mergedAt":null}]"#).is_empty()); // no head: skipped
        // closed unmerged: dead — null, missing and gh zero time alike
        assert_eq!(dead(r#"[{"headRefName":"a","mergedAt":null}]"#), ["a"]);
        assert_eq!(dead(r#"[{"headRefName":"a"}]"#), ["a"]);
        assert_eq!(
            dead(r#"[{"headRefName":"a","mergedAt":"0001-01-01T00:00:00Z"}]"#),
            ["a"]
        );
        // merged, or one of them landed: alive
        assert!(dead(r#"[{"headRefName":"a","mergedAt":"2026-09-27T04:50:08Z"}]"#).is_empty());
        assert!(
            dead(
                r#"[{"headRefName":"a","mergedAt":null},
                    {"headRefName":"a","mergedAt":"2026-09-27T04:50:08Z"}]"#
            )
            .is_empty()
        );
        // a live open PR, alone or after an earlier closed one: alive
        assert!(dead(r#"[{"headRefName":"a","state":"OPEN","mergedAt":null}]"#).is_empty());
        assert!(
            dead(
                r#"[{"headRefName":"a","state":"CLOSED","mergedAt":null},
                    {"headRefName":"a","state":"OPEN","mergedAt":null}]"#
            )
            .is_empty()
        );
        // branches are judged apart, from one list
        assert_eq!(
            dead(
                r#"[{"headRefName":"a","state":"CLOSED","mergedAt":null},
                    {"headRefName":"b","state":"OPEN","mergedAt":null}]"#
            ),
            ["a"]
        );
    }
}
