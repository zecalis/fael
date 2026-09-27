//! `[Merged]` (PLAN-fael-row-hygiene chunk 9): local branches whose PR
//! already merged but still exist — the branch of another session that had
//! to be traced by hand to a merged PR. Split out of `maintain.rs` next to
//! `orphan.rs`: the `gh` half must never live in core (core spawns no
//! processes).
//!
//! This file owns the single `gh pr list --state merged` call. `shipped`
//! (durable-log chunk 2) reads the same rows for their `mergedAt`/`number`
//! instead of spawning `gh` a second time.

use std::collections::BTreeMap;
use std::path::Path;

use crate::core;

/// One merged PR off a branch: when it landed plus its number for the close
/// command. `at` is `None` when gh gave no usable time.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Merge {
    pub(super) at: Option<i64>,
    pub(super) number: Option<u64>,
}

/// The `[Merged]` doctor problem, if any local branch already merged
/// upstream but still exists — kept here (not in `maintain.rs`) so
/// `open_row_notes` stays under the 100-line function cap.
pub(super) fn problem(root: &Path, prs: &BTreeMap<String, Vec<Merge>>) -> Option<core::Problem> {
    let landed = rows(root, prs);
    if landed.is_empty() {
        return None;
    }
    Some(core::Problem {
        kind: core::ProblemKind::Merged,
        severity: core::Severity::Info,
        fixable: false,
        file: None,
        detail: format!(
            "{} local branch(es) already merged upstream but still exist — \
             safe to delete (e.g. {})",
            landed.len(),
            landed[..landed.len().min(5)]
                .iter()
                .map(|b| format!("`git branch -D {b}`"))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    })
}

/// Local branches that are gone upstream (their PR merged) but still sit in
/// this clone — safe to delete. The current branch and the default branch
/// (`origin/HEAD`, else `main`) never count.
/// The `prs` map comes from the one shared `gh` call; an empty map (no `gh`,
/// no auth, or no merged PR) leaves this silent, never an error.
fn rows(root: &Path, prs: &BTreeMap<String, Vec<Merge>>) -> Vec<String> {
    let mut local = vec![];
    for line in locals(root) {
        if !line.is_empty() {
            local.push(line);
        }
    }
    if local.is_empty() {
        return vec![];
    }
    let current = crate::git(root, &["symbolic-ref", "--short", "-q", "HEAD"]);
    let default = default_branch(root);
    let mut out: Vec<String> = local
        .into_iter()
        .filter(|b| prs.contains_key(b))
        .filter(|b| Some(b) != current.as_ref() && *b != default)
        .collect();
    out.sort();
    out
}

/// The remote's default branch from `origin/HEAD` (set by clone, or
/// `git remote set-head origin -a`); no remote or no pointer → `main`.
/// Shared with `shipped` (its `git branch --merged` source).
pub(super) fn default_branch(root: &Path) -> String {
    crate::git(
        root,
        &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"],
    )
    .and_then(|r| r.strip_prefix("origin/").map(str::to_string))
    .unwrap_or_else(|| "main".into())
}

/// Every local branch head, one per line.
fn locals(root: &Path) -> Vec<String> {
    crate::git(
        root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .unwrap_or_default()
    .lines()
    .map(str::to_string)
    .collect()
}

/// Branch → its merged PRs. None = unknown (no `gh`, it failed, or
/// unparseable output) — callers fall back to `git branch --merged`, like
/// Orphan stays silent without evidence.
///
/// `FAEL_GH_MERGED_JSON` short-circuits the spawn with canned output (tests
/// only — same reason as orphan's `FAEL_GH_JSON`: no fake survives Windows
/// `CreateProcess` or runners with a real `gh`).
pub(super) fn merged_prs(root: &Path) -> Option<BTreeMap<String, Vec<Merge>>> {
    let json = if let Ok(fake) = std::env::var("FAEL_GH_MERGED_JSON") {
        fake
    } else {
        let out = std::process::Command::new("gh")
            .args([
                "pr",
                "list",
                "--state",
                "merged",
                "--json",
                "headRefName,mergedAt,number",
                "--limit",
                "200",
            ])
            .current_dir(root)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    parse_merged(&json)
}

/// Pure half of the above: None on unparseable output (nothing to say), else
/// the PRs per branch head (possibly empty — no merged PR at all). Entries
/// without a head name are skipped; without a usable time they still count,
/// as timeless, for the `[Shipped?]` fallback.
fn parse_merged(json: &str) -> Option<BTreeMap<String, Vec<Merge>>> {
    let ps = serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .as_array()?
        .clone();
    let mut out: BTreeMap<String, Vec<Merge>> = BTreeMap::new();
    for p in &ps {
        let Some(name) = p.get("headRefName").and_then(|h| h.as_str()) else {
            continue;
        };
        let at = match p.get("mergedAt") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(s)) if s.starts_with("0001-") => None,
            Some(serde_json::Value::String(s)) => core::ts_ms(s),
            _ => None,
        };
        out.entry(name.to_string()).or_default().push(Merge {
            at,
            number: p.get("number").and_then(|n| n.as_u64()),
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::parse_merged;

    #[test]
    fn merged_parses_heads_times_and_numbers() {
        assert_eq!(parse_merged("not json"), None);
        assert_eq!(parse_merged("{}"), None);
        assert!(parse_merged("[]").is_some_and(|m| m.is_empty()));
        let m = parse_merged(
            r#"[{"headRefName":"feat/y","mergedAt":"2026-09-27T04:50:08Z","number":43}]"#,
        )
        .unwrap();
        let one = &m["feat/y"][0];
        assert_eq!(one.at, crate::core::ts_ms("2026-09-27T04:50:08Z"));
        assert_eq!(one.number, Some(43));
        // null, zero and missing times read as timeless, never as errors
        let m = parse_merged(
            r#"[{"headRefName":"a","mergedAt":null},{"headRefName":"b","mergedAt":"0001-01-01T00:00:00Z"},{"headRefName":"c"}]"#,
        )
        .unwrap();
        assert!(m.values().all(|v| v[0].at.is_none()));
        // rows without the head key contribute nothing, silently
        assert!(parse_merged(r#"[{"number":1}]"#).is_some_and(|m| m.is_empty()));
        // a head-only entry (the old shape) is still a key in the map
        assert!(
            parse_merged(r#"[{"headRefName":"feat/y"}]"#).is_some_and(|m| m.contains_key("feat/y"))
        );
    }
}
