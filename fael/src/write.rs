//! Write-side path integrity (PLAN-fael-path-integrity chunk 4): `fael add`
//! without `--files` inherits the files this session edited, and every files
//! entry is checked against evidence before the row is written.
//!
//! Core never sees the disk here (PLAN §4) — this module is the binary side:
//! it reads the hook's session edits and the worktree, and core only gets the
//! resulting list through the normal `add_row` path.

use crate::{core, hook};
use paths::root_relative;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

mod paths;

/// Optional fields for `add_row` — bundled so the arg count stays under the lint.
pub(crate) struct AddOpts {
    pub key: Option<String>,
    pub to: Option<String>,
    pub title: Option<String>,
    pub revisit: Option<String>,
    pub urgent: core::Urgent,
    pub supersedes: Option<String>,
    pub force: bool,
    /// Reject the shape faults the caller can fix in the same call
    /// (`core::shape_rejects`) before writing. Off for hook-filed rows: a
    /// capture line cannot carry a `--title`, so there it only warns.
    pub gate: bool,
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
        title,
        revisit,
        urgent,
        supersedes,
        force,
        gate,
    } = opts;
    let mut files = core::normalize_files(files_arg, &r.cwd, &r.root)?;
    let mut warns = root_relative(r, files_arg, &mut files);
    let log = crate::read(r);
    if files.is_empty() {
        files = crate::session::derive(&r.root, &log);
    }
    warns.extend(check(
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
    // the queue position resolves against the open issues (`--urgent` = back,
    // `--urgent-before` = just above that row); core rejects non-issues
    row.urgent = core::resolve_urgent(&log, &urgent)?;
    // self-heal (chunks 3b–d): a repeat on these files or this key, or an id
    // the text names, supersedes itself — one shared Verdict for the real
    // add, `add --dry-run` and MCP `dry_run`
    let evaluated =
        crate::selfheal::evaluate(&log, &st, &row, supersedes.as_deref(), r.cfg.cross_key);
    warns.extend(evaluated.heal.notes.clone());
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
    if gate && !force {
        // reject before the write, never warn after it: nobody went back
        core::add_gate(&row, &log, &r.cfg, evaluated.heal.supersedes.as_deref())?;
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
    let (pending, st, log) = prepare(r, kind, text, files_arg, opts)?;
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
    let (row, path, mut core_warns) = core::close_row(
        &r.fael,
        r.journal.as_deref(),
        &log,
        &r.cfg,
        &crate::stamp(r),
        id,
        why,
    )?;
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
    let (row, path, mut warns) = core::bump_row(
        &r.fael,
        r.journal.as_deref(),
        &log,
        &r.cfg,
        &crate::stamp(r),
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

/// Check a files list against evidence before the row is written. Accepted
/// silently, in order: anchor · glob · on disk · rename-resolvable · in this
/// session's edits · in git status (shell-made files the edit hook never saw).
///
/// A path with no evidence is rejected only when almost surely a typo (a
/// same-directory file within edit distance 2); anything else files with a
/// warning — deleted or not-yet-created files are legitimate rows. `force`
/// turns the rejection into a warning.
pub(crate) fn check(
    root: &Path,
    al: &core::Aliases,
    files: &[String],
    edits: &[(String, i64)],
    force: bool,
) -> Result<Vec<String>, String> {
    let in_edits: HashSet<&str> = edits.iter().map(|(p, _)| p.as_str()).collect();
    let mut missing: Vec<&str> = vec![];
    for f in files {
        if hook::is_anchor(f) || core::is_glob(f) || root.join(f).exists() {
            continue;
        }
        if al.forward(f).iter().any(|p| root.join(p).exists()) {
            continue;
        }
        if in_edits.contains(f.as_str()) {
            continue;
        }
        missing.push(f.as_str());
    }
    if missing.is_empty() {
        return Ok(vec![]);
    }
    // one spawn, only when something is missing from disk — the add path
    // already spawns git for the stamp, so this costs nothing on success
    let status = git_status_files(root);
    missing.retain(|f| !status.contains(*f));
    let mut bad: Vec<(&str, String)> = vec![];
    let mut warns: Vec<String> = vec![];
    for f in missing {
        match sibling_suggest(root, f) {
            Some(near) if !force => bad.push((f, near)),
            _ => warns.push(format!(
                "warning: {f:?} matches nothing on disk — filed anyway; a typo? else its rows push once it exists"
            )),
        }
    }
    if !bad.is_empty() {
        let (f, near) = &bad[0];
        let mut msg = format!(
            "rejected: {f:?} matches nothing — not on disk, no rename leads to it, \
not in this session's edits or git status — did you mean {near:?}? \
(a file you have not created yet: add --force)"
        );
        if bad.len() > 1 {
            msg.push_str(&format!(" (+{} more)", bad.len() - 1));
        }
        return Err(msg);
    }
    Ok(warns)
}

/// Paths git knows in the worktree but the disk walk above missed: untracked,
/// staged and modified files (`git mv` without commit stages the new name).
/// Empty on any failure — fail-open, the caller just has less evidence.
fn git_status_files(root: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(s) = crate::git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    ) else {
        return out;
    };
    // `-z` entries are `XY <path>`; a rename or copy (`R`/`C` in X) is
    // followed by its source as a bare entry — the new name is the evidence
    let mut it = s.split('\0');
    while let Some(e) = it.next() {
        let Some(p) = e.get(3..).filter(|p| !p.is_empty()) else {
            continue;
        };
        if e.starts_with(['R', 'C']) {
            it.next();
        }
        out.insert(p.to_string());
    }
    out
}

/// A same-directory file on disk close enough that `bad` is almost surely a
/// typo of it — `None` when the parent directory is unreadable or nothing is
/// within distance 2. Only siblings count: a row file one char away may be a
/// genuinely new file (`src/a.rs` exists, `src/b.rs` is planned work), and
/// rejecting that is the false block the plan forbids.
fn sibling_suggest(root: &Path, bad: &str) -> Option<String> {
    let dir = bad.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let sib = if dir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    let Ok(rd) = std::fs::read_dir(&sib) else {
        return None;
    };
    let mut best: Option<(usize, String)> = None;
    for e in rd.flatten().take(500) {
        let n = e.file_name().to_string_lossy().replace('\\', "/");
        let cand = if dir.is_empty() {
            n
        } else {
            format!("{dir}/{n}")
        };
        if cand == bad {
            continue;
        }
        let d = core::levenshtein(bad, &cand);
        if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            best = Some((d, cand));
        }
    }
    let (d, cand) = best?;
    (d <= 2 && d < bad.chars().count()).then_some(cand)
}
