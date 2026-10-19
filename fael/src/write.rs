//! Write-side path integrity (PLAN-fael-path-integrity chunk 4): `fael add`
//! without `--files` inherits the files this session edited, and every files
//! entry is checked against evidence before the row is written.
//!
//! Core never sees the disk here (PLAN §4) — this module is the binary side:
//! it reads the hook's session edits and the worktree, and core only gets the
//! resulting list through the normal `add_row` path.

use crate::{core, hook};
use from::{from_user, over_user};
use paths::root_relative;
pub(crate) use paths::sibling_suggest;
use std::path::PathBuf;

pub(crate) mod arity;
mod cap;
mod flake;
mod from;
mod keys;
mod paths;
mod recall;

/// Optional fields for `add_row` — bundled so the arg count stays under the lint.
pub(crate) struct AddOpts {
    pub key: Option<String>,
    pub to: Option<String>,
    /// `user` when the user said or decided it; absent = the agent chose.
    pub from: Option<String>,
    pub title: Option<String>,
    pub revisit: Option<String>,
    pub urgent: core::Urgent,
    pub supersedes: Option<String>,
    pub force: bool,
}

/// A row built, healed and validated — everything short of the write. Shared
/// by `add_row` and dry runs, so a preview can never drift from the real add.
pub(crate) struct Pending {
    pub row: core::Row,
    pub evaluated: crate::selfheal::Evaluated,
    pub warns: Vec<String>,
}

/// Build + heal + validate without writing (normalise · check · self-heal ·
/// provenance · id scan). Dry runs stop here, so the previewed Verdict is
/// the one `add_row` would act on.
pub(crate) fn prepare(
    r: &crate::Repo,
    kind: &str,
    text: &str,
    files_arg: &[String],
    opts: AddOpts,
) -> Result<(Pending, core::Stamp, core::Log), String> {
    let AddOpts {
        key,
        to,
        from,
        title,
        revisit,
        urgent,
        supersedes,
        force,
    } = opts;
    let mut files = core::normalize_files(files_arg, &r.cwd, &r.root)?;
    let mut warns = root_relative(r, files_arg, &mut files);
    let mut log = crate::read(r);
    if files.is_empty() {
        files = crate::session::derive(&r.root, &log);
        // a long session's edits would push the row at every one of them:
        // the caller names the few it is about (PLAN-fael-context-loop §5b)
        let cap = crate::filehash::MAX_FILES;
        if files.len() > cap && !force {
            return Err(format!(
                "rejected: nothing written — {} files derived from this session's edits (over {cap}); name the ones the row is about with --files (or add --force to file it on all): {}",
                files.len(),
                files.join(",")
            ));
        }
    }
    warns.extend(paths::check(
        &r.root,
        &crate::aliases::load(r, &log, true),
        &files,
        &crate::session::active_edits(&r.root),
        force,
    )?);
    let st = crate::stamp(r);
    let mut row = core::Row::new(&st.by, kind, text, files);
    crate::session::tag_writer(&r.root, &mut row); // "written by A, used by B" needs the writer
    // the file as it stood when the row was written; one info line for any
    // real file left out (over 16 MiB, unreadable, past the 8th) — never a reject
    warns.extend(crate::filehash::stamp_row(&r.root, &mut row));
    row.key = key;
    // a headline lists show; the body stays in `text` for `find <id>` / `--full`
    row.title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    // when to look at this row again: a date kickoff surfaces, or free text
    row.revisit = revisit
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    // everything identity-like is lowercase: `--to Delamind` stores `delamind`
    row.to = to
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty());
    row.from = from_user(from)?;
    // the queue position resolves against the open issues (`--urgent` = back,
    // `--urgent-before` = just above that row); core rejects non-issues
    row.urgent = core::resolve_urgent(&log, &urgent)?;
    // self-heal (chunks 3b–d): a repeat on these files or this key, or an id
    // the text names, supersedes itself — one shared Verdict for the real
    // add, `add --dry-run` and MCP `dry_run`
    let evaluated =
        crate::selfheal::evaluate(&log, &st, &row, supersedes.as_deref(), r.cfg.cross_key);
    warns.extend(evaluated.heal.notes.clone());
    warns.extend(over_user(&log, &row, evaluated.heal.supersedes.as_deref()));
    // a flake repeat merges onto the same row (write/flake.rs): the count rides
    // the merged write, so the board shows how often it fired.
    if let Some(target) = evaluated.verdict.target()
        && row.kind == "issue"
        && flake::is_merge(&log, &row, target)
    {
        let n = log
            .rows
            .iter()
            .find(|x| x.id == target)
            .map(|x| flake::count(&x.text) + 1)
            .unwrap_or(2);
        if let Some((text, note)) = flake::stamped(&row, n) {
            row.text = text;
            warns.push(note);
        }
    }
    // verdict chunk 3: provenance for restore — which rule filed `supersedes`
    row.decision_source = evaluated.heal.source.clone();
    // id-refs chunk 2: prose citing an id with no row behind it says so —
    // one info line per id, never a reject. Skips the row itself and its
    // supersede target (a caller flag may name either verbatim).
    let mut skip = vec![row.id.as_str()];
    if let Some(s) = evaluated.heal.supersedes.as_deref() {
        skip.push(s);
    }
    warns.extend(phantom_lines(
        r,
        &log,
        row.title.as_deref(),
        &row.text,
        &skip,
    ));
    // (e) auto-key: the one key these files already carry. After `heal` on
    // purpose — a key fael guessed must never close a row through (c)
    if row.key.is_none()
        && let Some(k) = crate::selfheal::auto_key(&log, &row.files)
    {
        warns.push(format!("key {k} — the only key on these files"));
        row.key = Some(k);
    }
    // the write-time checks too, so a dry run rejects what the add would
    core::validate(&row, &r.cfg)?;
    if !force {
        // reject before the write, never warn after it: nobody went back
        core::add_gate(&row, &log, &r.cfg, evaluated.heal.supersedes.as_deref())?;
        cap::check(r, &mut log, &row, evaluated.heal.supersedes.as_deref())?;
    }
    let out = Pending {
        row,
        evaluated,
        warns,
    };
    Ok((out, st, log))
}

/// Core's add path over `prepare`'s build — shared by the CLI and MCP.
pub(crate) fn add_row(
    r: &crate::Repo,
    kind: &str,
    text: &str,
    files_arg: &[String],
    opts: AddOpts,
) -> Result<(core::Row, PathBuf, Vec<String>), String> {
    let (mut pending, st, log) = prepare(r, kind, text, files_arg, opts)?;
    crate::session::tag_superseder(
        &r.root,
        &log,
        &mut pending.row,
        pending.evaluated.heal.supersedes.as_deref(),
    );
    let mut warns = pending.warns;
    let (row, path, mut core_warns) = core::add_row(
        &r.fael,
        r.journal.as_deref(),
        &log,
        &r.cfg,
        &st,
        pending.row,
        pending.evaluated.heal.supersedes.as_deref(),
    )?;
    warns.append(&mut core_warns);
    warns.extend(recall::lines(&log, &row)); // closed earlier on these files (context-loop 5)
    warns.extend(keys::lines(&log, &row));
    // PLAN-fael-languages chunk 2: the row-language warning lives in core
    // (`lang::row_language_check` behind `[lang] rows`) — never a reject, one
    // warning line; under the default the string is byte-identical to the old one.
    if let Some(w) = core::row_language_check(&r.cfg, row.title.as_deref(), &row.text) {
        warns.push(w);
    }
    // chunk 6e: the id just filed is already in this session's context — mark
    // it seen so the next push does not repeat it; the turn's receipt counts it
    hook::note_filed(&r.root, &row);
    Ok((row, path, warns))
}

/// Id citations with no row behind them (PLAN-fael-id-refs chunk 2) — one
/// info line per id, never a reject. The title is prose a reader sees in
/// every list, so a citation there counts too. No `warning:` prefix on
/// purpose: lines without it pass through `record_asks`/`record_mcp`
/// uncounted, like the self-heal notes.
fn phantom_lines(
    r: &crate::Repo,
    log: &core::Log,
    title: Option<&str>,
    text: &str,
    skip: &[&str],
) -> Vec<String> {
    let prose = [title.unwrap_or(""), text].join(" ");
    crate::refs::phantoms(r, log, &prose, skip)
        .into_iter()
        .map(|tok| format!("no row with id {tok} — cited in the text; copy ids from fael find"))
        .collect()
}

/// `fael close` — resolve the target, stamp, append the close row. The reason
/// is scanned like `add_row`'s text (skipping the target); the row closes
/// regardless of what the scan finds.
pub(crate) fn close_row(
    r: &crate::Repo,
    id: &str,
    why: &str,
) -> Result<(core::Row, PathBuf, Vec<String>), String> {
    let log = crate::read(r);
    let mut warns = phantom_lines(r, &log, None, why, &[id]);
    warns.extend(crate::unmerged::lines(&r.root, why));
    let stamp = crate::session::retire_stamp(r, &log, id);
    let (row, path, mut core_warns) =
        core::close_row(&r.fael, r.journal.as_deref(), &log, &r.cfg, &stamp, id, why)?;
    warns.append(&mut core_warns);
    hook::note_closed(&r.root, &row);
    Ok((row, path, warns))
}

/// `fael bump <id>` — new `to`/`urgent`/`revisit` on the same row, under its id (a bump event).
/// At most one of `--urgent` (back of the queue), `--urgent-before <id>`
/// (just above that row), `--not-urgent` (leave the queue); none keeps the
/// old number. Absent `--revisit` keeps the old date/text; a value sets it.
pub(crate) fn bump(
    r: &crate::Repo,
    a: &crate::Args,
    id: &str,
) -> Result<(core::Row, PathBuf, Vec<String>), String> {
    let urgent = match (a.has("urgent"), a.one("urgent-before"), a.has("not-urgent")) {
        (false, None, false) => core::UrgentChange::Keep,
        (true, None, false) => core::UrgentChange::End,
        (false, Some(t), false) => core::UrgentChange::Before(t),
        (false, None, true) => core::UrgentChange::Remove,
        _ => {
            return Err(
                "rejected: bump takes at most one of --urgent, --urgent-before, --not-urgent"
                    .into(),
            );
        }
    };
    // bare `--revisit` names no date or text — that only filters on `find`
    let revisit = a.revisit_value()?;
    let log = crate::read(r);
    let moves =
        a.one("to").is_some() || revisit.is_some() || !matches!(urgent, core::UrgentChange::Keep);
    let (fh, note) = crate::filehash::for_bump(&r.root, &log, id, moves)?;
    let stamp = crate::session::retire_stamp(r, &log, id);
    let (row, path, mut warns) = core::bump_row(
        &r.fael,
        r.journal.as_deref(),
        &log,
        &r.cfg,
        &stamp,
        id,
        core::BumpOpts {
            to: a.one("to"),
            urgent,
            revisit,
            held: None,
            fh: Some(fh),
        },
    )?;
    warns.extend(note);
    Ok((row, path, warns))
}
