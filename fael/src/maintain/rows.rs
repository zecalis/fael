//! The open-row checks behind `doctor` (Gone/PartGone/Stale/Orphan), plus the
//! Merged/Shipped/Fat notes that build on the same rows. Split out of
//! `maintain.rs` for the 400-line cap; the `gh` half of Orphan and the merge
//! evidence of Merged/Shipped live here, never in core (core spawns no
//! processes). Every row-based note carries the full ids a cleanup agent
//! needs (`doctor --json` prints them) — not just the abbreviated examples.

use super::{alive, drift, fat, merged, noverdict, orphan, phantom, shipped, unstamped};
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
    let mut out = files_notes(log, root, al);
    out.extend(branch_notes(log));
    // a supersede marker whose newest version is already closed: the whole
    // chain is hidden with no close row — a pre-chain-close binary's trap
    out.extend(superseded_note(log));
    // per-rule self-heal precision from restore labels — shown only when a
    // label lands (re-adds alone never count); the judging lives in core
    if let Some(p) = core::doctor_precision(log) {
        out.push(p);
    }
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

/// Gone/PartGone/Stale: what each row's `files[]` (or backticked pointer)
/// still holds on disk.
fn files_notes(log: &core::Log, root: &Path, al: &core::Aliases) -> Vec<core::Problem> {
    let mut out = vec![];
    // a missing file that lives on the row's own unmerged branch is not gone
    let branches = alive::BranchFiles::new(root);
    let missing: Vec<(&core::Row, Vec<&str>)> = core::find(log, &core::Filter::default())
        .into_iter()
        .map(|row| (row, branches.missing(row, core::gone_files(root, row, al))))
        .filter(|(_, m)| !m.is_empty())
        .collect();
    // every file gone — the same test as `core::gone`, after the branch check
    let all_gone = |row: &core::Row, m: &[&str]| m.len() == row.files.len();
    let gone: Vec<&core::Row> = missing
        .iter()
        .filter(|(row, m)| all_gone(row, m))
        .map(|(row, _)| *row)
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
    let w = core::abbrev(log);
    let part: Vec<(String, String)> = missing
        .iter()
        .filter(|(row, m)| !all_gone(row, m))
        .map(|(row, m)| {
            (
                row.id.clone(),
                format!("{} → {}", w.short(&row.id), m.join(", ")),
            )
        })
        .collect();
    if !part.is_empty() {
        let eg: Vec<&str> = part.iter().take(5).map(|(_, e)| e.as_str()).collect();
        out.push(
            core::Problem::info(
                core::ProblemKind::PartGone,
                format!(
                    "{} open row(s) still name a file that no longer exists — check the text still \
                     holds, then re-file with `--supersedes` or `fael close <id>` (e.g. {})",
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
                     still holds, then re-file with `--supersedes` or `fael close <id>` (e.g. {})",
                    stale.len(),
                    eg.join("; ")
                ),
            )
            .with_ids(stale.iter().map(|(id, _)| id.clone()).collect()),
        );
    }
    // id rot: prose — in open rows, close texts and any `*.md` under the repo
    // — cites an id with no row behind it (id-refs chunk 3 + ids:doctor-plans);
    // a closed row's own text stays quiet
    out.extend(phantom::problems(log, root));
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
                 the work likely died with the branch; re-file with `--supersedes` or `fael close <id>` (e.g. {})",
                eg.join("; ")
            ),
        )
        .with_ids(ids),
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
    // the row that supersedes this one — kept only when its entire forward
    // chain is closed, so a live newer version is never called stuck
    let head_closed = |id: &str| {
        let mut newest = id;
        let mut seen = HashSet::from([newest]);
        while let Some(n) = log
            .rows
            .iter()
            .find(|r| r.supersedes.as_deref() == Some(newest))
        {
            if !seen.insert(n.id.as_str()) {
                break;
            }
            newest = n.id.as_str();
        }
        closed.contains(newest)
    };
    let stuck: Vec<&core::Row> = hidden
        .iter()
        .copied()
        .filter(|r| head_closed(r.id.as_str()))
        .collect();
    if stuck.is_empty() {
        return None;
    }
    let eg: Vec<String> = stuck
        .iter()
        .take(5)
        .map(|r| {
            format!(
                "{} → `fael close {} \"superseded\"`",
                w.short(&r.id),
                w.short(&r.id)
            )
        })
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
        .with_ids(stuck.iter().map(|r| r.id.clone()).collect()),
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
    let eg: Vec<&str> = hits.iter().take(5).map(|(_, e)| e.as_str()).collect();
    Some(
        core::Problem::info(
            core::ProblemKind::NotEnglish,
            format!(
                "{} open row(s) are not in {langs} — translate and re-file with \
                 `fael add --supersedes <id>` (e.g. {})",
                hits.len(),
                eg.join("; ")
            ),
        )
        .with_ids(hits.iter().map(|(id, _)| id.clone()).collect()),
    )
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
