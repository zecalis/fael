//! Gone/PartGone/Stale: what each open row's `files[]` (or backticked pointer)
//! still holds on disk, plus the `[Phantom]` id rot. Split out of `rows.rs`
//! for the 400-line cap. A missing file names the git fact that explains it
//! (`removed`), never a verdict: closing or re-filing stays the reader's call.

use super::{alive, phantom, removed};
use crate::core;
use std::path::Path;

/// A row and the files it names that are missing from the checkout.
type Missing<'a> = (&'a core::Row, Vec<&'a str>);

/// Gone/PartGone/Stale/Phantom: what each row's `files[]` (or backticked
/// pointer, or cited id) still holds.
pub(super) fn notes(log: &core::Log, root: &Path, al: &core::Aliases) -> Vec<core::Problem> {
    // a missing file that lives on the row's own unmerged branch is not gone
    let branches = alive::BranchFiles::new(root);
    let missing: Vec<Missing> = core::find(log, &core::Filter::default())
        .into_iter()
        .map(|row| (row, branches.missing(row, core::gone_files(root, row, al))))
        .filter(|(_, m)| !m.is_empty())
        .collect();
    let mut out = missing_notes(log, root, &missing);
    out.extend(stale_note(log, root, al));
    // id rot: prose — in open rows, close texts and any `*.md` under the repo
    // — cites an id with no row behind it (id-refs chunk 3 + ids:doctor-plans);
    // a closed row's own text stays quiet
    out.extend(phantom::problems(log, root));
    out
}

/// `[Gone]` (every file missing) and `[PartGone]` (some left), each missing
/// file with the git fact behind it — one `git log` for the whole set.
fn missing_notes(log: &core::Log, root: &Path, missing: &[Missing]) -> Vec<core::Problem> {
    let mut out = vec![];
    let all: Vec<&str> = missing
        .iter()
        .flat_map(|(_, m)| m.iter().copied())
        .collect();
    let why = if all.is_empty() {
        Default::default()
    } else {
        removed::why(root, &all)
    };
    let named = |m: &[&str]| {
        m.iter()
            .map(|f| {
                why.get(*f)
                    .map_or((*f).to_string(), |w| format!("{f} — {w}"))
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let w = core::abbrev(log);
    // every file gone — the same test as `core::gone`, after the branch check
    let (gone, part): (Vec<&Missing>, Vec<&Missing>) = missing
        .iter()
        .partition(|(row, m)| m.len() == row.files.len());
    if !gone.is_empty() {
        let (eg, more) = core::split_shown(
            gone.iter()
                .map(|(row, m)| {
                    format!(
                        "{} ({}): `fael close {} \"gone\"`",
                        w.short(&row.id),
                        named(m),
                        w.short(&row.id)
                    )
                })
                .collect(),
        );
        // mechanically closable: every file is gone from every branch, so the
        // row never pushes again — `doctor --fix` applies exactly the ready
        // command above (force, same identity rule as the close path)
        let closes: Vec<(String, String)> = gone
            .iter()
            .map(|(row, _)| (row.id.clone(), "gone".into()))
            .collect();
        out.push(
            core::Problem::info(
                core::ProblemKind::Gone,
                format!(
                    "{} open row(s) name only files that no longer exist, so they never push — \
                     re-file them on the new path or close them (e.g. {})",
                    gone.len(),
                    eg.join("; ")
                ),
            )
            .with_closes(closes)
            .with_more(more),
        );
    }
    // some files gone, some left: the row still pushes, but it likely describes
    // the repo as it was (a tool swapped out, a config file removed)
    if !part.is_empty() {
        let (eg, more) = core::split_shown(
            part.iter()
                .map(|(row, m)| {
                    format!(
                        "{} ({}): `fael close {} \"part-gone\"`",
                        w.short(&row.id),
                        named(m),
                        w.short(&row.id)
                    )
                })
                .collect(),
        );
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
            .with_ids(part.iter().map(|(row, _)| row.id.clone()).collect())
            .with_more(more),
        );
    }
    out
}

/// `[Stale]` — prose rot: the row's text points at a backticked path with no
/// file behind it, so the next reader follows a dead pointer. The example is a
/// ready command (same id the gate resolves); an issue still owes its
/// `<cause> → <fix>` + guard — the detector never invents them.
fn stale_note(log: &core::Log, root: &Path, al: &core::Aliases) -> Option<core::Problem> {
    let stale = stale_rows(log, root, al);
    if stale.is_empty() {
        return None;
    }
    let w = core::abbrev(log);
    let (eg, more) = core::split_shown(
        stale
            .iter()
            .map(|(id, e)| format!("{e}: `fael close {} \"stale\"`", w.short(id)))
            .collect(),
    );
    Some(
        core::Problem::info(
            core::ProblemKind::Stale,
            format!(
                "{} open row(s) name a path in backticks that is not on disk — check the text \
                 still holds, then re-file with `--supersedes` or close it (e.g. {})",
                stale.len(),
                eg.join("; ")
            ),
        )
        .with_ids(stale.iter().map(|(id, _)| id.clone()).collect())
        .with_more(more),
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
