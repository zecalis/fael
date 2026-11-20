//! The open-row checks behind `doctor` (Gone/PartGone/Stale/Orphan), plus the
//! Merged/Shipped/Fat notes that build on the same rows (`files` holds the
//! Gone/PartGone/Stale half). Split out of
//! `maintain.rs` for the 400-line cap; the `gh` half of Orphan and the merge
//! evidence of Merged/Shipped live here, never in core (core spawns no
//! processes). Every row-based note carries the full ids a cleanup agent
//! needs (`doctor --json` prints them) — not just the abbreviated examples.

use super::{drift, fat, files, merged, noverdict, orphan, shipped, unstamped};
use crate::core;
use std::collections::HashSet;
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
    let mut out = files::notes(log, root, al);
    out.extend(branch_notes(log));
    // a supersede marker whose newest version is already closed: the whole
    // chain is hidden with no close row — a pre-chain-close binary's trap
    out.extend(superseded_note(log));
    // per-rule self-heal precision from restore labels — shown only when a
    // label lands (re-adds alone never count); the judging lives in core
    if let Some(p) = core::doctor_precision(log) {
        out.push(p);
    }
    // one `gh pr list --state all` call feeds the three `gh` checks below:
    // `[Merged]` uses the branch set, `[Shipped]` the mergedAt/number per
    // branch, `[Orphan]` the state per branch — unknown when `gh` is
    // unavailable, never an error
    let prs_opt = merged::merged_prs(root);
    if prs_opt.is_none() || orphan::unavailable() {
        // the `gh` checks ran on no evidence: say so once, so the silence
        // never reads as clean
        out.push(core::Problem::info(
            core::ProblemKind::GhOffline,
            "gh unavailable: merged/shipped/orphan checks skipped".into(),
        ));
    }
    let prs = prs_opt.unwrap_or_default();
    // landed branches: merged upstream but still sitting in this clone, so
    // the next reader keeps wondering whether the work is done
    out.extend(merged::problem(root, &prs));
    // shipped notes: open notes filed on a landed branch — the work is done
    // but the note still pushes (`doctor --fix` closes them, the adapter side)
    out.extend(shipped::problems(log, root, &prs));
    // fat rows: the add-time warnings the agent skipped, repeated per row so
    // one topic per row can still be superseded alone; pre-self-heal legacy
    // rows stay collapsed to one line (`doctor --json` lists every row id)
    out.extend(fat::problem(log, cfg, expand_fat));
    // rows not in the accepted `[lang] rows` scripts: the add-time warning
    // repeated as one batch, so a translate pass can supersede them all
    // (`fael add --json -`) — closed/superseded rows never appear
    out.extend(not_english_note(log, cfg));
    // the safety net for rows the code outgrew: many commits on a row's
    // files since it was written — the reader checks each against the code
    out.extend(drift::problem(log, root));
    // rows on a file over the push cap: `fh` is stamped but push never
    // compares it, so the generic hint stands — one `metadata()` per file
    out.extend(noverdict::problem(log, root));
    // rows with no `fh` on a file push could compare: no stamp, no verdict —
    // a bare `fael bump` restamps once the reader checked it is still true
    out.extend(unstamped::problem(log, root));
    out
}

/// Orphan: open rows filed on a branch whose PR died unmerged. One shared
/// `gh pr list --state all` call for the whole repo (`orphan::rows`, the same
/// list `[Merged]` and `[Shipped]` read).
fn branch_notes(log: &core::Log) -> Vec<core::Problem> {
    let orphan = orphan::rows(log);
    if orphan.is_empty() {
        return vec![];
    }
    let w = core::abbrev(log);
    let n: usize = orphan.iter().map(|(_, ids)| ids.len()).sum();
    let ids: Vec<String> = orphan.iter().flat_map(|(_, i)| i.iter().cloned()).collect();
    let (eg, more) = core::split_shown(
        orphan
            .iter()
            .map(|(b, i)| {
                let shorts: Vec<&str> = i.iter().map(|id| w.short(id)).collect();
                let first = shorts.first().copied().unwrap_or("?");
                format!(
                    "{b} → {}: `fael close {first} \"orphan\"`",
                    shorts.join(", ")
                )
            })
            .collect(),
    );
    vec![
        core::Problem::info(
            core::ProblemKind::Orphan,
            format!(
                "{n} open row(s) filed on branch(es) whose PR was closed without merge — \
                 the work likely died with the branch; re-file with `--supersedes` or close it (e.g. {})",
                eg.join("; ")
            ),
        )
        .with_ids(ids)
        .with_more(more),
    ]
}

/// `[Superseded]`: rows hidden only by another row's `supersedes` marker whose
/// newest version is already closed — the whole chain is unreachable (no close
/// row on the old versions, and `fael close` on the newest one is "already
/// closed"). A pre-chain-close binary's trap; `fael close <id>` on each old
/// version repairs it (the relaxed guard lets it through once the head is
/// closed). This is the one note whose fix is `fael close`, never `--supersedes`:
/// re-filing would only grow another chain.
fn superseded_note(log: &core::Log) -> Option<core::Problem> {
    let closed = core::closed(log);
    let sup = core::superseded(log);
    let hidden: Vec<&core::Row> = log
        .rows
        .iter()
        .filter(|r| sup.contains(r.id.as_str()) && !closed.contains(r.id.as_str()))
        .collect();
    if hidden.is_empty() {
        return None;
    }
    let w = core::abbrev(log);
    // the row that supersedes this one — kept only when every forward path
    // is closed, so a live newer version on any branch is never called stuck
    // (a diamond — two rows superseding one parent — walks every child, not
    // just the first)
    let head_closed = |id: &str| {
        let mut stack = vec![id];
        let mut seen = HashSet::from([id]);
        let mut heads = 0;
        while let Some(cur) = stack.pop() {
            let children: Vec<&str> = log
                .rows
                .iter()
                .filter(|r| r.supersedes.as_deref() == Some(cur))
                .map(|r| r.id.as_str())
                .collect();
            if children.is_empty() {
                // a head of the forward graph: one live head means the row
                // still has a newer version
                heads += 1;
                if !closed.contains(cur) {
                    return false;
                }
            } else {
                for c in children {
                    if seen.insert(c) {
                        stack.push(c);
                    }
                }
            }
        }
        // no head at all is a pure supersede cycle: never stuck (no live
        // version, but no closable head either)
        heads > 0
    };
    let stuck: Vec<&core::Row> = hidden
        .iter()
        .copied()
        .filter(|r| head_closed(r.id.as_str()))
        .collect();
    if stuck.is_empty() {
        return None;
    }
    let (eg, more) = core::split_shown(
        stuck
            .iter()
            .map(|r| {
                format!(
                    "{} → `fael close {} \"superseded\"`",
                    w.short(&r.id),
                    w.short(&r.id)
                )
            })
            .collect(),
    );
    // mechanically closable: the head is already closed, so the relaxed guard
    // lets each stuck version through — `doctor --fix` applies exactly the
    // ready command above (force, same identity rule as the close path)
    let closes: Vec<(String, String)> = stuck
        .iter()
        .map(|r| (r.id.clone(), "superseded".into()))
        .collect();
    Some(
        core::Problem::info(
            core::ProblemKind::Superseded,
            format!(
                "{} row(s) are hidden only by a supersede marker whose newest version is already \
                 closed — close them to finish the chain (e.g. {})",
                stuck.len(),
                eg.join("; ")
            ),
        )
        .with_closes(closes)
        .with_more(more),
    )
}

/// `(full id, short-id → files)` for every open row whose title or text
/// carries a letter outside every accepted `[lang] rows` script — the same
/// `lang::row_language_check` the add path warns with, so doctor repeats the
/// skipped warning as one batch with the full ids a translate pass needs.
fn not_english_note(log: &core::Log, cfg: &core::Config) -> Option<core::Problem> {
    let hits: Vec<(String, String)> = core::find(log, &core::Filter::default())
        .into_iter()
        .filter(|row| core::row_language_check(cfg, row.title.as_deref(), &row.text).is_some())
        .map(|row| {
            let w = core::abbrev(log);
            (
                row.id.clone(),
                format!("{} → {}", w.short(&row.id), row.files.join(", ")),
            )
        })
        .collect();
    if hits.is_empty() {
        return None;
    }
    let langs = cfg.lang_rows.join("/");
    let (eg, more) = core::split_shown(hits.iter().map(|(_, e)| e.clone()).collect());
    Some(
        core::Problem::info(
            core::ProblemKind::NotEnglish,
            format!(
                "{} open row(s) are not in {langs} — chat in any language, rows stay in English — translate and re-file with \
                 `fael add --supersedes <id>` (e.g. {})",
                hits.len(),
                eg.join("; ")
            ),
        )
        .with_ids(hits.iter().map(|(id, _)| id.clone()).collect())
        .with_more(more),
    )
}
