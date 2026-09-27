//! `[Shipped]` (PLAN-fael-durable-log chunk 2): open notes filed on a branch
//! whose PR merged after the row was born — the work already landed, so the
//! note is stale. Split out of `maintain.rs` next to `orphan.rs`/`merged.rs`:
//! the `gh`/`git` half must never live in core (core spawns no processes).
//!
//! Judged by branch **name**, never sha: squash and rebase drop the row's sha
//! from main. The row's birth comes from its ULID time (`core::ulid_ms`,
//! falling back to `ts`); the merge time from the PR's `mergedAt`. A reused
//! branch name (every timed PR predates the row) stays silent; a landed
//! branch with no usable merge time reports `[Shipped?]` instead.

use crate::core;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// One merged PR off a branch: when it landed plus its number for the close
/// command. `at` is `None` when gh gave no usable time.
#[derive(Debug, Clone, PartialEq)]
struct Merge {
    at: Option<i64>,
    number: Option<u64>,
}

enum Verdict {
    Shipped(Option<u64>),
    Maybe(Option<u64>),
    No,
}

/// The `[Shipped]` + `[Shipped?]` doctor problems, if any open note sits on a
/// landed branch — kept here (not in `maintain.rs`) so `open_row_notes` stays
/// under the 100-line function cap.
pub(super) fn problems(log: &core::Log, root: &Path) -> Vec<core::Problem> {
    let prs = merged_prs(root).unwrap_or_default();
    let git = git_merged(root);
    if prs.is_empty() && git.is_empty() {
        return vec![];
    }
    let mut sure: Vec<String> = vec![];
    let mut maybe: Vec<String> = vec![];
    let w = core::abbrev(log);
    for row in core::find(log, &core::Filter::default()) {
        if row.kind != "note" {
            continue;
        }
        let Some(branch) = row.branch().filter(|b| !b.is_empty()) else {
            continue;
        };
        let short = row.id[..w.min(row.id.len())].to_string();
        match decide(prs.get(branch), git.contains(branch), birth_ms(row)) {
            Verdict::Shipped(n) => sure.push(entry(&short, branch, n)),
            Verdict::Maybe(n) => maybe.push(entry(&short, branch, n)),
            Verdict::No => {}
        }
    }
    let mut out = vec![];
    if !sure.is_empty() {
        out.push(core::Problem {
            kind: core::ProblemKind::Shipped,
            severity: core::Severity::Info,
            fixable: false,
            file: None,
            detail: format!(
                "{} open note(s) already landed with their branch — \
                 close them (e.g. {})",
                sure.len(),
                sure[..sure.len().min(5)].join("; ")
            ),
        });
    }
    if !maybe.is_empty() {
        out.push(core::Problem {
            kind: core::ProblemKind::ShippedMaybe,
            severity: core::Severity::Info,
            fixable: false,
            file: None,
            detail: format!(
                "{} open note(s) on branch(es) that look merged but have no \
                 merge time to confirm — check, then `fael close` (e.g. {})",
                maybe.len(),
                maybe[..maybe.len().min(5)].join("; ")
            ),
        });
    }
    out
}

/// `short-id (branch #N): `fael close …`` — the close text names the PR when
/// its number is known, plain `shipped` when it is not.
fn entry(short: &str, branch: &str, number: Option<u64>) -> String {
    match number {
        Some(n) => format!("{short} ({branch} #{n}): `fael close {short} \"shipped in #{n}\"`"),
        None => format!("{short} ({branch}): `fael close {short} \"shipped\"`"),
    }
}

/// Pure half of `problems`: the earliest PR merged at/after the row's birth
/// wins (`Shipped`); every timed PR predating the row means a reused branch
/// name (`No`); anything landed but timeless is unconfirmed (`Maybe`).
fn decide(prs: Option<&Vec<Merge>>, in_git: bool, birth: Option<u64>) -> Verdict {
    let mut after: Vec<&Merge> = vec![];
    let mut before_known = false;
    let mut unknown = false;
    let mut first_number: Option<u64> = None;
    if let Some(list) = prs {
        for m in list {
            first_number = first_number.or(m.number);
            match (m.at, birth) {
                (Some(at), Some(b)) if (at.max(0) as u64) >= b => after.push(m),
                (Some(_), Some(_)) => before_known = true,
                _ => unknown = true,
            }
        }
    }
    if !after.is_empty() {
        after.sort_by_key(|m| (m.at.unwrap_or(i64::MAX), m.number.unwrap_or(u64::MAX)));
        return Verdict::Shipped(after[0].number);
    }
    if before_known && !unknown {
        return Verdict::No;
    }
    if prs.is_some_and(|l| !l.is_empty()) || in_git {
        return Verdict::Maybe(first_number);
    }
    Verdict::No
}

/// The row's birth in unix ms: the ULID time first, the `ts` field as fallback.
fn birth_ms(row: &core::Row) -> Option<u64> {
    core::ulid_ms(&row.id).or_else(|| core::ts_ms(&row.ts).and_then(|t| u64::try_from(t).ok()))
}

/// Branch → its merged PRs. None = unknown (no `gh`, it failed, or
/// unparseable output) — the caller falls back to `git branch --merged`,
/// like Orphan stays silent without evidence.
///
/// `FAEL_GH_MERGED_JSON` short-circuits the spawn with canned output (tests
/// only — same reason as merged's seam: no fake survives Windows
/// `CreateProcess` or runners with a real `gh`).
fn merged_prs(root: &Path) -> Option<BTreeMap<String, Vec<Merge>>> {
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

/// Local branches merged into the default branch (no merge time — the
/// `[Shipped?]` source). Empty when git fails; never an error.
fn git_merged(root: &Path) -> HashSet<String> {
    let default = super::merged::default_branch(root);
    crate::git(
        root,
        &["branch", "--format=%(refname:short)", "--merged", &default],
    )
    .unwrap_or_default()
    .lines()
    .map(str::to_string)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{Merge, Verdict, decide, parse_merged};

    fn merge(at: Option<i64>, number: Option<u64>) -> Merge {
        Merge { at, number }
    }

    fn decided(prs: Option<Vec<Merge>>, in_git: bool, birth: Option<u64>) -> String {
        match decide(prs.as_ref(), in_git, birth) {
            Verdict::Shipped(n) => format!("shipped:{n:?}"),
            Verdict::Maybe(n) => format!("maybe:{n:?}"),
            Verdict::No => "no".into(),
        }
    }

    #[test]
    fn shipped_when_pr_merged_after_row_birth() {
        // squash fixture (§3): the row's sha never reaches main, the branch
        // name plus mergedAt still prove it landed
        let prs = vec![merge(Some(1_758_864_000_000), Some(43))];
        assert_eq!(
            decided(Some(prs), false, Some(1_758_800_000_000)),
            "shipped:Some(43)"
        );
    }

    #[test]
    fn silent_when_branch_name_reused_after_merge() {
        // every timed PR predates the row: a new branch under an old name
        let prs = vec![merge(Some(1_700_000_000_000), Some(41))];
        assert_eq!(decided(Some(prs), true, Some(1_758_800_000_000)), "no");
    }

    #[test]
    fn earliest_pr_after_birth_wins() {
        let prs = vec![
            merge(Some(1_758_900_000_000), Some(45)),
            merge(Some(1_758_864_000_000), Some(43)),
            merge(Some(1_700_000_000_000), Some(41)),
        ];
        assert_eq!(
            decided(Some(prs), false, Some(1_758_800_000_000)),
            "shipped:Some(43)"
        );
    }

    #[test]
    fn maybe_without_merge_time() {
        // git-only (no PR entry at all) and timeless PRs both stay unconfirmed
        assert_eq!(decided(None, true, Some(1_758_800_000_000)), "maybe:None");
        let prs = vec![merge(None, Some(43))];
        assert_eq!(
            decided(Some(prs), false, Some(1_758_800_000_000)),
            "maybe:Some(43)"
        );
    }

    #[test]
    fn silent_without_any_evidence() {
        assert_eq!(decided(None, false, Some(1_758_800_000_000)), "no");
        assert_eq!(decided(Some(vec![]), false, Some(1)), "no");
    }

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
    }
}
