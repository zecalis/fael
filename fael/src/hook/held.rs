//! Held ask (PLAN-fael-row-retire w1, from PLAN-fael-decision-held chunk 2): an
//! open decision or note — any writer — naming a file this session edited,
//! whose file moved since the row was written, may still be true, be what the
//! code holds now, or be wrong. Stop stashes those ids; the next push names
//! them once per session with the three verdicts ready to run. The agent
//! judges — fael only names the pair it saw, and an unjudged row stays active.
//! `fael chunk done` prints the same over the files the chunk touched (w2).

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

/// The ids stop stashed for the next push, one `<id>\t<path>` line each.
fn path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.held"))
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
/// already told. A sub-agent's stop never reaches here.
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
    Some(Line {
        kind: Kind::Held {
            ids: live.iter().map(|(r, _)| r.id.clone()).collect(),
        },
        text: text(&c.log, &live),
    })
}

/// `fael chunk done`: the held text over the files the chunk touched, at most
/// `DONE_MAX` rows, then where the rest are. Empty when none.
pub(crate) fn done_text(r: &crate::Repo, files: &[String]) -> String {
    let mut log = crate::read(r);
    super::changed::mark_dormant(r, &mut log);
    let al = crate::aliases::load(r, &log, true);
    // decision 01M5528X: the Held line at Stop is the one ask over a changed
    // row; this list leaves the dormant ones out
    let mut found = pairs(&log, &r.root, &al, "", files);
    found.retain(|(w, _)| !log.dormant.contains(&w.id));
    let (head, rest) = found.split_at(found.len().min(DONE_MAX));
    let mut out = text(&log, head);
    if !rest.is_empty() {
        let mut more: Vec<&str> = rest.iter().map(|(_, f)| f.as_str()).collect();
        more.sort_unstable();
        more.dedup();
        out.push_str(&format!(
            "+{} more: fael find --files {}\n",
            rest.len(),
            more.join(",")
        ));
    }
    out
}
