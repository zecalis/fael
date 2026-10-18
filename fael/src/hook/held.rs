//! Held ask (PLAN-fael-row-retire w1, from PLAN-fael-decision-held chunk 2): an
//! open decision or note — any writer — naming a file this session edited,
//! whose file moved since the row was written, may still be true, be what the
//! code holds now, or be wrong. Stop stashes those ids; the next push names
//! them once per session with the three verdicts ready to run. The agent
//! judges — fael only names the pair it saw, and an unjudged row stays active.
//! `fael chunk done` prints the same over the files the chunk touched (w2);
//! `fael chunk start` over the files under the chunk's scope (c3).

use super::changed::{Blobs, moved, own_row, user_note};
use super::protocol::Ctx;
use super::say::{Kind, Line};
use super::state::{edited_files, seen_path, session_key, state_dir};
use crate::core;
use std::path::{Path, PathBuf};

/// The seen-list line of the session's one held line: stop reads it too, so
/// a session already told stashes nothing more.
pub(crate) const KEY: &str = "~held";

// ponytail: a session that edited more names only the first few; the rest stay
// active and keep pushing — raise it if `fael stats` held yield says so
const MAX: usize = 3;

/// `chunk done` names more: its diff is the whole chunk, not one session's turn.
const DONE_MAX: usize = 5;

/// `fael add` names fewer: the row's own files, not the chunk's diff.
const ADD_MAX: usize = 3;

/// The ids stop stashed for the next push, one `<id>\t<path>` line each.
fn path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.held"))
}

/// Per-session file of where each retired row was shown: one
/// `<source>\t<id>` line each (`held`, `done`, later `add`/`start`). Keyed
/// like `path` but through `session_id`, so a transcript path and its stem
/// meet — the push speaks the former, `writer_session` the latter. Fail open
/// everywhere: an unreadable file is no sources, an unwritable one drops.
pub(crate) fn shown_path(session: &str, root: &Path) -> PathBuf {
    let id = super::state::session_id(session);
    let key = session_key(&format!("{id}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.shown"))
}

/// Append one `<source>\t<id>` line per id. No-op on empty session or ids.
pub(crate) fn record_shown(session: &str, root: &Path, source: &str, ids: &[String]) {
    if session.is_empty() || ids.is_empty() {
        return;
    }
    let p = shown_path(session, root);
    if p.parent()
        .is_none_or(|d| std::fs::create_dir_all(d).is_err())
    {
        return;
    }
    let body: String = ids.iter().map(|id| format!("{source}\t{id}\n")).collect();
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(body.as_bytes())
        });
}

/// Sorted unique sources that named `target` in this session's shown file.
pub(crate) fn shown_for(session: &str, root: &Path, target: &str) -> Vec<String> {
    let Ok(s) = std::fs::read_to_string(shown_path(session, root)) else {
        return vec![];
    };
    let mut out: Vec<String> = s
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, id)| *id == target)
        .map(|(src, _)| src.to_string())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// True for a row neither closed nor superseded.
fn active(log: &core::Log) -> impl Fn(&str) -> bool {
    let (closed, gone) = (core::closed(log), core::superseded(log));
    move |id| !closed.contains(id) && !gone.contains(id)
}

/// Each active decision or note naming a file of `edited` that moved since the
/// row was written, with that file: `session`'s own rows first, then log order.
/// Issues stay out — they have the edit ask.
pub(crate) fn pairs<'a>(
    log: &'a core::Log,
    root: &Path,
    al: &core::Aliases,
    session: &str,
    edited: &[String],
) -> Vec<(&'a core::Row, String)> {
    let active = active(log);
    let mut blobs = Blobs::new();
    let mut found: Vec<(&core::Row, String)> = log
        .rows
        .iter()
        .filter(|r| matches!(r.kind.as_str(), "decision" | "note") && active(&r.id))
        .filter_map(|r| Some((r, moved(r, root, al, edited, &mut blobs)?)))
        .collect();
    found.sort_by_key(|(r, _)| !own_row(r, session));
    found
}

/// One line per row with its three verdicts ready to run (short ids); a
/// `from: user` row says to ask the user first; the chain hint once when there
/// are several.
pub(crate) fn text(log: &core::Log, rows: &[(&core::Row, String)]) -> String {
    let ab = core::abbrev(log);
    let mut out: String = rows
        .iter()
        .map(|(r, f)| {
            let (id, user) = (ab.short(&r.id), user_note(std::iter::once(*r)));
            let title = r.title.as_deref().map(|t| format!(" \"{t}\"")).unwrap_or_default();
            format!(
                "fael: {} {id}{title} — `{f}` changed since it was written · still true? fael bump {id} · the code holds it? fael close {id} \"now in `{f}`\" · wrong now? re-file with --supersedes {id}{user}\n",
                r.kind
            )
        })
        .collect();
    if rows.len() > 1 {
        out.push_str("fael: chain several in one shell call with &&\n");
    }
    out
}

/// At stop: stash the session's held candidates, unless the session was
/// already told. A sub-agent's stop never reaches here. A row this session
/// already showed (an `add`, `chunk done` or `chunk start` listed it) is
/// left out, so the Held line names only the rows still unshown.
pub(super) fn stash(c: &Ctx) {
    if c.session.is_empty() {
        return;
    }
    let seen = std::fs::read_to_string(seen_path(&c.session, "", &c.repo.root));
    if seen.is_ok_and(|s| s.lines().any(|l| l == KEY)) {
        return;
    }
    let edited = edited_files(&c.session, &c.repo.root, usize::MAX);
    if edited.is_empty() {
        return;
    }
    let al = crate::aliases::load(&c.repo, &c.log, false);
    let mut found = pairs(&c.log, &c.repo.root, &al, &c.session, &edited);
    // c2: add no longer spends ~held — it stamps shown instead, so the Held
    // line keeps the rows add never named and drops only the ones it did
    found.retain(|(r, _)| shown_for(&c.session, &c.repo.root, &r.id).is_empty());
    found.truncate(MAX);
    let p = path(&c.session, &c.repo.root);
    if found.is_empty()
        || p.parent()
            .is_none_or(|d| std::fs::create_dir_all(d).is_err())
    {
        return;
    }
    let body: String = found
        .iter()
        .map(|(r, f)| format!("{}\t{f}\n", r.id))
        .collect();
    let _ = std::fs::write(p, body);
}

/// At the next push: the held line for the stashed ids still active — one
/// closed or superseded since stop is not named. `None` = nothing to say.
pub(super) fn line(c: &Ctx) -> Option<Line> {
    let s = std::fs::read_to_string(path(&c.session, &c.repo.root)).ok()?;
    let active = active(&c.log);
    let live: Vec<(&core::Row, String)> = s
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(id, _)| active(id))
        .filter_map(|(id, f)| Some((c.log.rows.iter().find(|r| r.id == id)?, f.to_string())))
        .collect();
    if live.is_empty() {
        return None;
    }
    let ids: Vec<String> = live.iter().map(|(r, _)| r.id.clone()).collect();
    Some(Line {
        kind: Kind::Held { ids },
        text: text(&c.log, &live),
    })
}

/// `fael chunk done`: the held text over the files the chunk touched, at most
/// `DONE_MAX` rows, then where the rest are. Empty when none.
pub(crate) fn done_text(r: &crate::Repo, files: &[String]) -> String {
    let mut log = crate::read(r);
    super::changed::mark_dormant(r, &mut log);
    let al = crate::aliases::load(r, &log, true);
    let found = select(&log, &r.root, &al, files, "");
    let (head, rest) = found.split_at(found.len().min(DONE_MAX));
    let mut out = text(&log, head);
    out.push_str(&more_line(rest));
    // r1: stamp what this output showed, so a later retire names it — the
    // head only, never the `+N more` tail. The writer session is the caller's
    // (fail open: none outside one).
    if !head.is_empty()
        && let Some(s) = crate::session::writer_session(&r.root)
    {
        let ids: Vec<String> = head.iter().map(|(w, _)| w.id.clone()).collect();
        record_shown(&s, &r.root, "done", &ids);
    }
    out
}

/// `fael chunk start` (plan fael-row-retire c3): the same held text over the
/// files under the chunk's declared scope — the one moment every dev chunk
/// passes. `scope` holds the declared scope lines (dirs or files); a row
/// counts when one of its files sits under the scope on a `/` boundary.
/// Same selection and wording as `done_text` (`select` + `text` + `more_line`),
/// at most `DONE_MAX` rows, then where the rest are. Silent when none. Never
/// blocks start, never closes anything.
pub(crate) fn start_text(r: &crate::Repo, scope: &[String]) -> String {
    use std::collections::HashSet;
    if scope.is_empty() {
        return String::new();
    }
    let mut log = crate::read(r);
    super::changed::mark_dormant(r, &mut log);
    let al = crate::aliases::load(r, &log, true);
    // the scope names dirs (`w/`) while a row names files (`w/a.rs`): the
    // edited set is the row files under the scope, so `pairs` (exact match)
    // sees the same rows `overlap` would.
    let mut seen = HashSet::new();
    let mut edited: Vec<String> = vec![];
    for row in &log.rows {
        for f in &row.files {
            if fael_core::plan::overlap(std::slice::from_ref(f), scope) && seen.insert(f.clone()) {
                edited.push(f.clone());
            }
        }
    }
    if edited.is_empty() {
        return String::new();
    }
    let found = select(&log, &r.root, &al, &edited, "");
    let (head, rest) = found.split_at(found.len().min(DONE_MAX));
    let mut out = text(&log, head);
    out.push_str(&more_line(rest));
    // r1: stamp what this output showed under `start`, so a later retire of
    // one of these rows carries `shown: ["start", …]` — head only, fail open.
    if !head.is_empty()
        && let Some(s) = crate::session::writer_session(&r.root)
    {
        let ids: Vec<String> = head.iter().map(|(w, _)| w.id.clone()).collect();
        record_shown(&s, &r.root, "start", &ids);
    }
    out
}

/// `fael add` (CLI and MCP): the held text over the files the new row was
/// filed on, at most `ADD_MAX` rows, then where the rest are — the same
/// selection `done_text` uses, never a second one. Empty when none. The new
/// row itself is excluded: add never asks about what it just wrote.
///
/// Command output, not a hook line (no Outbox kind): the caller prints the
/// returned text after the add result. Each named row is stamped shown
/// `add`, so a later close/bump of it carries `shown: ["add", …]`, and
/// `held::stash` skips a row this session already showed — the Stop Held
/// line names only the rest, never a row add already named. A row already
/// shown this session (held, done, start or an earlier add) is not named
/// again.
pub(crate) fn add_text(r: &crate::Repo, files: &[String], exclude: &str) -> String {
    let mut log = crate::read(r);
    let al = crate::aliases::cached(r, &log).unwrap_or_default();
    super::changed::mark_dormant_with(r, &mut log, &al);
    let mut found = select(&log, &r.root, &al, files, exclude);
    // once per session per row: a row this session was already shown is done
    if let Some(s) = crate::session::writer_session(&r.root) {
        found.retain(|(w, _)| shown_for(&s, &r.root, &w.id).is_empty());
    }
    let (head, rest) = found.split_at(found.len().min(ADD_MAX));
    if head.is_empty() {
        return String::new();
    }
    let mut out = text(&log, head);
    out.push_str(&more_line(rest));
    let ids: Vec<String> = head.iter().map(|(w, _)| w.id.clone()).collect();
    if let Some(s) = crate::session::writer_session(&r.root) {
        record_shown(&s, &r.root, "add", &ids);
    }
    out
}

/// The open decisions/notes `done_text` and `add_text` share: any writer's,
/// on a file of `files`, whose stamped blob differs from disk, live only
/// (dormant rows stay out), minus `exclude` (add's own new row). Issues stay
/// out — they have the edit ask.
fn select<'a>(
    log: &'a core::Log,
    root: &Path,
    al: &core::Aliases,
    files: &[String],
    exclude: &str,
) -> Vec<(&'a core::Row, String)> {
    let mut found = pairs(log, root, al, "", files);
    // decision 01M5528X: the Held line at Stop is the one ask over a changed
    // row; this list leaves the dormant ones out
    found.retain(|(w, _)| !log.dormant.contains(&w.id) && w.id != exclude);
    found
}

/// `+N more: fael find --files …` over the unshown tail, or empty.
fn more_line(rest: &[(&core::Row, String)]) -> String {
    if rest.is_empty() {
        return String::new();
    }
    let mut more: Vec<&str> = rest.iter().map(|(_, f)| f.as_str()).collect();
    more.sort_unstable();
    more.dedup();
    format!(
        "+{} more: fael find --files {}\n",
        rest.len(),
        more.join(",")
    )
}
