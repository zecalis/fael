//! `[Orphan]` (PLAN-fael-row-hygiene chunk 6): open rows filed on a branch
//! whose PRs all closed unmerged. Split out of `maintain.rs` for the 400-line
//! cap, and because the `gh` half must never live in core (core spawns no
//! processes).

use crate::core;

/// `branch → short-id(s)` for every branch on open rows whose PRs all closed
/// unmerged — a live open PR off the branch is not orphan (review finding).
/// One `gh` call per branch; no `gh`, no auth, or no PR for the branch →
/// skipped silently, never an error.
pub(super) fn rows(log: &core::Log) -> Vec<(String, Vec<String>)> {
    let w = core::abbrev(log);
    let mut by_branch: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for row in core::find(log, &core::Filter::default()) {
        if let Some(b) = row.branch().filter(|b| !b.is_empty()) {
            by_branch
                .entry(b.to_string())
                .or_default()
                .push(row.id[..w.min(row.id.len())].to_string());
        }
    }
    by_branch
        .into_iter()
        .filter(|(b, _)| pr_closed_unmerged(b).is_some_and(|u| u))
        .collect()
}

/// None = unknown (no `gh`, it failed, or no PR off this branch);
/// Some(true) = every PR off this branch closed unmerged (the work died);
/// Some(false) = at least one PR is open or merged (the branch is alive).
///
/// `--state all` so an open PR off the branch is seen — a branch with a fresh
/// open PR is not orphan just because an earlier PR closed unmerged.
///
/// `FAEL_GH_JSON` short-circuits the spawn with canned output (tests only —
/// Windows `CreateProcess` never resolves a `.bat` fake off PATH, and CI
/// runners ship a real `gh` that answers on its own, so no fake survives
/// there; cf. `FAEL_STATE_DIR`).
fn pr_closed_unmerged(branch: &str) -> Option<bool> {
    if let Ok(fake) = std::env::var("FAEL_GH_JSON") {
        return pr_all_unmerged(&fake);
    }
    let out = std::process::Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "all",
            "--head",
            branch,
            "--json",
            "state,mergedAt",
            "--limit",
            "100",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    pr_all_unmerged(&String::from_utf8_lossy(&out.stdout))
}

/// Pure half of the above: None on unparseable output or an empty list (no PR
/// off this branch → nothing to say). An `OPEN` PR means the branch is still
/// alive → Some(false); otherwise Some(true) when every listed PR is unmerged.
/// A missing/null `mergedAt` is unmerged; so is gh's zero time `0001-…`.
fn pr_all_unmerged(json: &str) -> Option<bool> {
    let ps = serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .as_array()?
        .clone();
    if ps.is_empty() {
        return None;
    }
    if ps
        .iter()
        .any(|p| p.get("state").and_then(|s| s.as_str()) == Some("OPEN"))
    {
        return Some(false);
    }
    Some(ps.iter().all(|p| match p.get("mergedAt") {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.starts_with("0001-"),
        _ => false,
    }))
}

#[cfg(test)]
mod tests {
    use super::pr_all_unmerged;

    #[test]
    fn orphan_parses_gh_state_and_merged_at() {
        assert_eq!(pr_all_unmerged("[]"), None); // no PR: nothing to say
        assert_eq!(pr_all_unmerged("not json"), None);
        assert_eq!(pr_all_unmerged("{}"), None);
        assert_eq!(
            pr_all_unmerged(r#"[{"mergedAt":null}]"#),
            Some(true) // closed unmerged: orphan
        );
        assert_eq!(
            pr_all_unmerged(r#"[{"number":1}]"#),
            Some(true) // key missing entirely: orphan
        );
        assert_eq!(
            pr_all_unmerged(r#"[{"mergedAt":"0001-01-01T00:00:00Z"}]"#),
            Some(true) // gh zero time: orphan
        );
        assert_eq!(
            pr_all_unmerged(r#"[{"mergedAt":"2026-09-27T04:50:08Z"}]"#),
            Some(false) // merged: not orphan
        );
        assert_eq!(
            pr_all_unmerged(r#"[{"mergedAt":null},{"mergedAt":"2026-09-27T04:50:08Z"}]"#),
            Some(false) // one of them landed: not orphan
        );
        assert_eq!(
            pr_all_unmerged(r#"[{"state":"OPEN","mergedAt":null}]"#),
            Some(false) // a live open PR: not orphan
        );
        assert_eq!(
            pr_all_unmerged(
                r#"[{"state":"CLOSED","mergedAt":null},{"state":"OPEN","mergedAt":null}]"#
            ),
            Some(false) // an earlier closed PR plus a live one: not orphan
        );
    }
}
