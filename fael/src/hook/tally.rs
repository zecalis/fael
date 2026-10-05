//! The user channel's ledger (PLAN-fael-visible-secretary chunk 4): one
//! append-only file per session and worktree, beside the seen list. `add`,
//! `close` and the push append what they did (`filed <id>`, `closed <id>`,
//! `retired <id>`, `reminded <id>`, `whispered <file>`); Stop reads the
//! lines since its last `turn` marker into the receipt, then appends one.
//! Per-machine state only — nothing here reaches a row or the agent.

use super::protocol::Ctx;
use super::state::seen_path;
use crate::core;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `<seen file>.tally` — keyed like the seen list (session id stem +
/// worktree, the session's own thread), so `add` in the shell and the hook
/// land in one file.
fn path(session: &str, root: &Path) -> PathBuf {
    seen_path(session, "", root).with_extension("tally")
}

/// Append `<what> <item>` lines. Empty session = no-op (outside any hook
/// session nobody gets a receipt). Fails open, like every state write.
pub(crate) fn note(session: &str, root: &Path, what: &str, items: &[&str]) {
    if session.is_empty() || items.is_empty() {
        return;
    }
    let p = path(session, root);
    let out: String = items.iter().map(|i| format!("{what} {i}\n")).collect();
    let _ = p
        .parent()
        .map(std::fs::create_dir_all)
        .and_then(Result::ok)
        .and_then(|_| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .ok()
        })
        .map(|mut f| f.write_all(out.as_bytes()));
}

/// `add` (CLI, MCP, capture): the row is in this session's context now —
/// the seen list keeps the next push from repeating it (chunk 6e) — and on
/// the turn's receipt, with the version it retired.
pub(crate) fn note_filed(root: &Path, row: &core::Row) {
    let session = crate::session::hook_session(root);
    // before the seen list takes `row` — and never for the evaluator's own
    // stage-change row: fael would be counting its own write as evidence
    if let (Some(old), false) = (
        row.supersedes.as_deref(),
        row.key.as_deref() == Some(super::stage::KEY),
    ) {
        super::cited::note_dup(&session, root, old);
    }
    super::state::note_seen(&session, root, &[&row.id]);
    note(&session, root, "filed", &[&row.id]);
    if let Some(old) = row.supersedes.as_deref() {
        note(&session, root, "retired", &[old]);
    }
}

/// `close`: the closed row goes on the turn's receipt.
pub(crate) fn note_closed(root: &Path, row: &core::Row) {
    if let Some(target) = row.reference.as_deref() {
        note(
            &crate::session::hook_session(root),
            root,
            "closed",
            &[target],
        );
    }
}

/// True the first time this session notes `<what> <item>` — and records it,
/// so the same notice shows once per session.
pub(crate) fn first(session: &str, root: &Path, what: &str, item: &str) -> bool {
    let line = format!("{what} {item}");
    if session.is_empty() || lines(session, root).lines().any(|l| l == line) {
        return false;
    }
    note(session, root, what, &[item]);
    true
}

fn lines(session: &str, root: &Path) -> String {
    std::fs::read_to_string(path(session, root)).unwrap_or_default()
}

/// True the first time this session whispers about any of `files` — and
/// records them, so the next push on the same file stays quiet (≤1 per file
/// per session).
// ponytail: read-then-append without a lock — two parallel reads of one file
// may both whisper once; the seen lock already keeps them to distinct rows
pub(crate) fn first_whisper(session: &str, root: &Path, files: &[String]) -> bool {
    if session.is_empty() {
        return false;
    }
    let all = lines(session, root);
    let done = |f: &str| all.lines().any(|l| l.strip_prefix("whispered ") == Some(f));
    let fresh: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|f| !done(f))
        .collect();
    note(session, root, "whispered", &fresh);
    !fresh.is_empty()
}

/// Stop: the turn's receipt from the lines after the last `turn` marker,
/// then a new marker. `None` when the turn did nothing worth a line — fael
/// says nothing rather than "nothing new".
pub(crate) fn take_receipt(session: &str, root: &Path, log: &core::Log) -> Option<String> {
    if session.is_empty() {
        return None;
    }
    let all = lines(session, root);
    let turn = all.rsplit("turn -\n").next().unwrap_or("");
    if turn.is_empty() {
        return None;
    }
    note(session, root, "turn", &["-"]);
    receipt(turn, log)
}

/// `fael: this turn — filed 2 (01M3V2YQM, 01M3V2YQN) · reminded 1 (#k) · …`.
/// Each segment names up to two ids `fael find` takes back — every credit
/// points at something checkable; a repeated id counts once.
fn receipt(turn: &str, log: &core::Log) -> Option<String> {
    let ab = core::abbrev(log);
    let label = |id: &str| -> String {
        match log
            .rows
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.key.clone())
        {
            Some(k) => format!("#{k}"),
            None => ab.short(id).to_string(),
        }
    };
    let mut parts = vec![];
    for (what, word) in [
        ("filed", "filed"),
        ("reminded", "reminded"),
        ("retired", "retired"),
        ("closed", "closed"),
    ] {
        let mut ids: Vec<&str> = turn
            .lines()
            .filter_map(|l| l.strip_prefix(what)?.strip_prefix(' '))
            .collect();
        let mut seen = std::collections::HashSet::new();
        ids.retain(|i| seen.insert(*i));
        if ids.is_empty() {
            continue;
        }
        let named: Vec<String> = ids.iter().take(2).map(|i| label(i)).collect();
        let more = if ids.len() > 2 { " …" } else { "" };
        parts.push(format!("{word} {} ({}{more})", ids.len(), named.join(", ")));
    }
    (!parts.is_empty()).then(|| format!("fael: this turn — {}", parts.join(" · ")))
}

/// PLAN-fael-visible-secretary chunk 4: the user hears which decision or
/// issue the agent was just reminded of — one line, the first such row, at
/// most once per file per session. Every reminded id also goes to the
/// tally for the turn's receipt. Notes and repo kinds stay quiet: a
/// reminder is a choice made or a problem known, never a row count.
pub(super) fn whisper(c: &Ctx, said: &[&core::Row], files: &[String]) -> Option<String> {
    if !c.repo.cfg.notify_user {
        return None;
    }
    let hits: Vec<&core::Row> = said
        .iter()
        .copied()
        .filter(|r| matches!(r.kind.as_str(), "decision" | "issue"))
        .collect();
    let ids: Vec<&str> = hits.iter().map(|r| r.id.as_str()).collect();
    note(&c.session, &c.repo.root, "reminded", &ids);
    let first = hits.first()?;
    if !first_whisper(&c.session, &c.repo.root, files) {
        return None;
    }
    let label = match &first.key {
        Some(k) => format!("#{k}"),
        None => core::abbrev(&c.log).short(&first.id).to_string(),
    };
    let more = match hits.len() {
        1 => String::new(),
        n => format!(" +{} more", n - 1),
    };
    Some(format!(
        "fael: reminded agent — {label} \"{}\" ({}){more}",
        first.display_title(),
        files.join(", ")
    ))
}
