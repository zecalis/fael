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

use super::merged::Merge;

enum Verdict {
    Shipped(Option<u64>),
    Maybe(Option<u64>),
    No,
}

/// One open note that sits on a landed branch. Keeps the full id (for
/// `doctor --json`) and the PR number (for the `--fix` close text) — the
/// abbreviated examples in `detail` are for the human eye only.
struct Landed {
    id: String,
    short: String,
    branch: String,
    number: Option<u64>,
}

impl Landed {
    /// `short-id (branch #N): fael close …` — the close command names the PR
    /// when its number is known, plain `shipped` when it is not.
    fn example(&self) -> String {
        match self.number {
            Some(n) => format!(
                "{} ({} #{n}): `fael close {} \"shipped in #{n}\"`",
                self.short, self.branch, self.short
            ),
            None => format!(
                "{} ({}): `fael close {} \"shipped\"`",
                self.short, self.branch, self.short
            ),
        }
    }

    /// The `why` `--fix` passes to `fael close` for this note.
    fn close_text(&self) -> String {
        match self.number {
            Some(n) => format!("shipped in #{n}"),
            None => "shipped".into(),
        }
    }
}

/// The `[Shipped]` + `[Shipped?]` doctor problems, if any open note sits on a
/// landed branch — kept here (not in `maintain.rs`) so `open_row_notes` stays
/// under the 100-line function cap. `[Shipped]` carries the close actions
/// `doctor --fix` applies (and so reads `[--fix]`); `[Shipped?]` never does.
pub(super) fn problems(
    log: &core::Log,
    root: &Path,
    prs: &BTreeMap<String, Vec<Merge>>,
) -> Vec<core::Problem> {
    let git = git_merged(root);
    if prs.is_empty() && git.is_empty() {
        return vec![];
    }
    let mut sure: Vec<Landed> = vec![];
    let mut maybe: Vec<Landed> = vec![];
    let w = core::abbrev(log);
    for row in core::find(log, &core::Filter::default()) {
        // a revisit or a plan handoff waits on something other than the
        // merge (PLAN-fael-close-helpers §2) — never counted, never --fix closed
        if row.kind != "note" || waits_past_merge(row) {
            continue;
        }
        let Some(branch) = row.branch().filter(|b| !b.is_empty()) else {
            continue;
        };
        let landed = |n| Landed {
            id: row.id.clone(),
            short: w.short(&row.id).to_string(),
            branch: branch.to_string(),
            number: n,
        };
        match decide(prs.get(branch), git.contains(branch), birth_ms(row)) {
            Verdict::Shipped(n) => sure.push(landed(n)),
            Verdict::Maybe(n) => maybe.push(landed(n)),
            Verdict::No => {}
        }
    }
    let mut out = vec![];
    if !sure.is_empty() {
        let eg: Vec<String> = sure.iter().take(5).map(Landed::example).collect();
        let closes: Vec<(String, String)> = sure
            .iter()
            .map(|l| (l.id.clone(), l.close_text()))
            .collect();
        out.push(
            core::Problem::info(
                core::ProblemKind::Shipped,
                format!(
                    "{} open note(s) already landed with their branch — \
                     close them (e.g. {})",
                    sure.len(),
                    eg.join("; ")
                ),
            )
            .with_closes(closes),
        );
    }
    if !maybe.is_empty() {
        let eg: Vec<String> = maybe.iter().take(5).map(Landed::example).collect();
        out.push(core::Problem::info(
            core::ProblemKind::ShippedMaybe,
            format!(
                "{} open note(s) on branch(es) that look merged but have no \
                 merge time to confirm — check, then `fael close` (e.g. {})",
                maybe.len(),
                eg.join("; ")
            ),
        ));
    }
    out
}

/// A note whose end is not its branch's merge: it carries a `revisit`, or it
/// is a plan's `*:handoff` (the next chunk reads it after this one merges).
fn waits_past_merge(row: &core::Row) -> bool {
    row.revisit.is_some() || row.key.as_deref().is_some_and(|k| k.ends_with(":handoff"))
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

/// Local branches merged into the default branch (no merge time — the
/// `[Shipped?]` source). The default branch is always merged into itself, so
/// it is dropped: a note filed on `main` did not ship on a branch. Empty when
/// git fails; never an error.
fn git_merged(root: &Path) -> HashSet<String> {
    let default = super::merged::default_branch(root);
    crate::git(
        root,
        &["branch", "--format=%(refname:short)", "--merged", &default],
    )
    .unwrap_or_default()
    .lines()
    .filter(|b| *b != default.as_str())
    .map(str::to_string)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{Merge, Verdict, decide};

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
}
