//! Held-by close (PLAN-fael-decision-held chunk 2): a decision this session
//! wrote on a file it then edited may be what the code holds now. Stop stashes
//! those ids; the next push names them once per session with a ready
//! `fael close <id> "now in <path>"`. The agent judges whether the code holds
//! it — fael only names the pair it saw, and an unclosed decision stays active.

use super::changed::own_row;
use super::protocol::Ctx;
use super::say::{Kind, Line};
use super::state::{edited_files, seen_path, session_key, state_dir};
use crate::core;
use std::path::{Path, PathBuf};

/// The seen-list line of the session's one held line: stop reads it too, so
/// a session already told stashes nothing more.
pub(crate) const KEY: &str = "~held";

// ponytail: a session that wrote more names only the first few; the rest stay
// active and keep pushing — raise it if `fael stats` held yield says so
const MAX: usize = 3;

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

/// `(id, path)` for each decision `session` wrote that is still active and
/// names a file in `edited` — the first such file.
fn pairs(log: &core::Log, session: &str, edited: &[String]) -> Vec<(String, String)> {
    let active = active(log);
    log.rows
        .iter()
        .filter(|r| r.kind == "decision" && own_row(r, session) && active(&r.id))
        .filter_map(|r| {
            let f = r.files.iter().find(|f| edited.contains(f))?;
            Some((r.id.clone(), f.clone()))
        })
        .take(MAX)
        .collect()
}

/// At stop: stash this session's held candidates, unless the session was
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
    let found = pairs(&c.log, &c.session, &edited);
    let p = path(&c.session, &c.repo.root);
    if found.is_empty()
        || p.parent()
            .is_none_or(|d| std::fs::create_dir_all(d).is_err())
    {
        return;
    }
    let body: String = found.iter().map(|(id, f)| format!("{id}\t{f}\n")).collect();
    let _ = std::fs::write(p, body);
}

/// At the next push: the held line for the stashed ids still active — one
/// closed or superseded since stop is not named. `None` = nothing to say.
pub(super) fn line(c: &Ctx) -> Option<Line> {
    let s = std::fs::read_to_string(path(&c.session, &c.repo.root)).ok()?;
    let active = active(&c.log);
    let live: Vec<(&str, &str)> = s
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .filter(|(id, _)| active(id))
        .collect();
    if live.is_empty() {
        return None;
    }
    let text = live
        .iter()
        .map(|(id, f)| format!("fael: decision {id} was written this session and `{f}` edited — if the code holds it now: fael close {id} \"now in `{f}`\"\n"))
        .collect();
    Some(Line {
        kind: Kind::Held {
            ids: live.iter().map(|(id, _)| id.to_string()).collect(),
        },
        text,
    })
}
