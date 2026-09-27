//! `[Merged]` (PLAN-fael-row-hygiene chunk 9): local branches whose PR
//! already merged but still exist — the branch of another session that had
//! to be traced by hand to a merged PR. Split out of `maintain.rs` next to
//! `orphan.rs`: the `gh` half must never live in core (core spawns no
//! processes).

use std::path::Path;

use crate::core;

/// The `[Merged]` doctor problem, if any local branch already merged
/// upstream but still exists — kept here (not in `maintain.rs`) so
/// `open_row_notes` stays under the 100-line function cap.
pub(super) fn problem(root: &Path) -> Option<core::Problem> {
    let landed = rows(root);
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
/// One `gh` call total; no `gh`, no auth, or no merged PR → empty, silently.
fn rows(root: &Path) -> Vec<String> {
    let mut local = vec![];
    for line in locals(root) {
        if !line.is_empty() {
            local.push(line);
        }
    }
    if local.is_empty() {
        return vec![];
    }
    let Some(merged) = merged_heads(root) else {
        return vec![];
    };
    let current = crate::git(root, &["symbolic-ref", "--short", "-q", "HEAD"]);
    let default = default_branch(root);
    let mut out: Vec<String> = local
        .into_iter()
        .filter(|b| merged.contains(b))
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

/// The set of branch heads with a merged PR. None = unknown (no `gh`, it
/// failed, or unparseable output) — the caller stays silent, like Orphan.
///
/// `FAEL_GH_MERGED_JSON` short-circuits the spawn with canned output (tests
/// only — same reason as orphan's `FAEL_GH_JSON`: no fake survives Windows
/// `CreateProcess` or runners with a real `gh`).
fn merged_heads(root: &Path) -> Option<std::collections::HashSet<String>> {
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
    parse_heads(&json)
}

/// Pure half of the above: None on unparseable output (nothing to say), else
/// the set of heads with a merged PR (possibly empty — no merged PR at all).
fn parse_heads(json: &str) -> Option<std::collections::HashSet<String>> {
    let ps = serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .as_array()?
        .clone();
    Some(
        ps.iter()
            .filter_map(|p| p.get("headRefName").and_then(|h| h.as_str()))
            .map(String::from)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_heads;

    #[test]
    fn merged_parses_gh_head_refs() {
        assert_eq!(parse_heads("not json"), None);
        assert_eq!(parse_heads("{}"), None);
        assert!(parse_heads("[]").is_some_and(|s| s.is_empty()));
        assert!(parse_heads(r#"[{"headRefName":"feat/y"}]"#).is_some_and(|s| s.contains("feat/y")));
        // rows without the key (a shape change) contribute nothing, silently
        assert!(parse_heads(r#"[{"number":1}]"#).is_some_and(|s| s.is_empty()));
    }
}
