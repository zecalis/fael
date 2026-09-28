//! The open-row checks behind `doctor` (Gone/PartGone/Stale/Orphan), plus the
//! Merged/Shipped/Fat notes that build on the same rows. Split out of
//! `maintain.rs` for the 400-line cap; the `gh` half of Orphan and the merge
//! evidence of Merged/Shipped live here, never in core (core spawns no
//! processes). Every row-based note carries the full ids a cleanup agent
//! needs (`doctor --json` prints them) — not just the abbreviated examples.

use super::{fat, merged, orphan, shipped};
use crate::core;
use std::path::Path;

/// Every open-row note `doctor` shows: what the rows still say versus what
/// the repo (and its PRs) still hold.
pub(super) fn open_row_notes(
    log: &core::Log,
    root: &Path,
    al: &core::Aliases,
    cfg: &core::Config,
    expand_fat: bool,
) -> Vec<core::Problem> {
    let mut out = files_notes(log, root, al);
    out.extend(branch_notes(log));
    // one `gh pr list --state merged` call feeds both checks: `[Merged]` uses
    // the branch set, `[Shipped]` the mergedAt/number per branch
    let prs = merged::merged_prs(root).unwrap_or_default();
    // landed branches: merged upstream but still sitting in this clone, so
    // the next reader keeps wondering whether the work is done
    out.extend(merged::problem(root, &prs));
    // shipped notes: open notes filed on a landed branch — the work is done
    // but the note still pushes (`doctor --fix` closes them, the adapter side)
    out.extend(shipped::problems(log, root, &prs));
    // fat rows: the add-time warnings the agent skipped, repeated per row so
    // one topic per row can still be superseded alone; pre-self-heal legacy
    // rows stay collapsed to one line unless `--fat` asks for the full list
    out.extend(fat::problem(log, cfg, expand_fat));
    out
}

/// Gone/PartGone/Stale: what each row's `files[]` (or backticked pointer)
/// still holds on disk.
fn files_notes(log: &core::Log, root: &Path, al: &core::Aliases) -> Vec<core::Problem> {
    let mut out = vec![];
    let gone: Vec<_> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter(|row| core::gone(root, row, al))
        .collect();
    if !gone.is_empty() {
        let w = core::abbrev(log);
        let eg: Vec<String> = gone
            .iter()
            .take(5)
            .map(|row| format!("{} → {}", w.short(&row.id), row.files.join(", ")))
            .collect();
        let ids = gone.iter().map(|row| row.id.clone()).collect();
        out.push(
            core::Problem::info(
                core::ProblemKind::Gone,
                format!(
                    "{} open row(s) name only files that no longer exist, so they never push — \
                     re-file them on the new path or `fael close` them (e.g. {})",
                    gone.len(),
                    eg.join("; ")
                ),
            )
            .with_ids(ids),
        );
    }
    // some files gone, some left: the row still pushes, but it likely describes
    // the repo as it was (a tool swapped out, a config file removed)
    let part: Vec<(String, String)> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter(|row| !core::gone(root, row, al))
        .filter_map(|row| {
            let g = core::gone_files(root, row, al);
            let w = core::abbrev(log);
            (!g.is_empty()).then(|| {
                (
                    row.id.clone(),
                    format!("{} → {}", w.short(&row.id), g.join(", ")),
                )
            })
        })
        .collect();
    if !part.is_empty() {
        let eg: Vec<&str> = part.iter().take(5).map(|(_, e)| e.as_str()).collect();
        out.push(
            core::Problem::info(
                core::ProblemKind::PartGone,
                format!(
                    "{} open row(s) still name a file that no longer exists — check the text still \
                     holds, then re-file with `--supersedes` or `fael close` (e.g. {})",
                    part.len(),
                    eg.join("; ")
                ),
            )
            .with_ids(part.iter().map(|(id, _)| id.clone()).collect()),
        );
    }
    // prose rot: the row's text points at a backticked path with no file
    // behind it, so the next reader follows a dead pointer
    let stale = stale_rows(log, root, al);
    if !stale.is_empty() {
        let eg: Vec<&str> = stale.iter().take(5).map(|(_, e)| e.as_str()).collect();
        out.push(
            core::Problem::info(
                core::ProblemKind::Stale,
                format!(
                    "{} open row(s) name a path in backticks that is not on disk — check the text \
                     still holds, then re-file with `--supersedes` or `fael close` (e.g. {})",
                    stale.len(),
                    eg.join("; ")
                ),
            )
            .with_ids(stale.iter().map(|(id, _)| id.clone()).collect()),
        );
    }
    out
}

/// Orphan: open rows filed on a branch whose PR died unmerged. One `gh` call
/// per branch (`orphan::rows`).
fn branch_notes(log: &core::Log) -> Vec<core::Problem> {
    let orphan = orphan::rows(log);
    if orphan.is_empty() {
        return vec![];
    }
    let w = core::abbrev(log);
    let n: usize = orphan.iter().map(|(_, ids)| ids.len()).sum();
    let ids: Vec<String> = orphan.iter().flat_map(|(_, i)| i.iter().cloned()).collect();
    let eg: Vec<String> = orphan
        .iter()
        .take(5)
        .map(|(b, i)| {
            let shorts: Vec<&str> = i.iter().map(|id| w.short(id)).collect();
            format!("{b} → {}", shorts.join(", "))
        })
        .collect();
    vec![
        core::Problem::info(
            core::ProblemKind::Orphan,
            format!(
                "{n} open row(s) filed on branch(es) whose PR was closed without merge — \
                 the work likely died with the branch; re-file with `--supersedes` or `fael close` (e.g. {})",
                eg.join("; ")
            ),
        )
        .with_ids(ids),
    ]
}

/// `(full id, short-id → dead backticked path(s))` for every open row whose
/// text still points at a path with no file behind it (row-hygiene chunk 4).
/// The full id rides along so `doctor --json` can hand a cleanup agent the
/// actionable id, not just the sketched example.
fn stale_rows(log: &core::Log, root: &Path, al: &core::Aliases) -> Vec<(String, String)> {
    core::find(log, &core::Filter::default())
        .into_iter()
        .filter_map(|row| {
            let refs = core::stale_refs(root, row, al);
            let w = core::abbrev(log);
            (!refs.is_empty()).then(|| {
                (
                    row.id.clone(),
                    format!("{} → {}", w.short(&row.id), refs.join(", ")),
                )
            })
        })
        .collect()
}
