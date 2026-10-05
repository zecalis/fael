//! The stop event. It files the reply's `fael <kind>:` lines (`capture`),
//! and stashes a bug announcement with no issue row for the next push to
//! show once. It never blocks: fael never starts an agent turn.

use super::capture;
use super::markers::{bug_signal_from_transcript, has_bug_marker};
use super::protocol::{Event, Reply, ctx};
use super::state::{file_birth_ms, now_rfc3339, risk_path, session_key, state_dir};
use crate::core;
use std::path::Path;

/// The stop event: `decide`, then the once-per-session sync and the receipt.
pub(crate) fn stop(e: &Event) -> Reply {
    if e.agent.is_some() {
        return subagent_stop(e);
    }
    let mut r = decide(e);
    super::autosync::start(e);
    r.notice = receipt(e);
    r
}

/// PLAN-fael-visible-secretary chunk 4: the turn's receipt for the user —
/// `None` when it did nothing worth a line.
// ponytail: resolves the repo and reads the log a second time after
// `decide`; thread the Ctx through if stop's latency budget ever needs it
fn receipt(e: &Event) -> Option<String> {
    let c = ctx(e)?;
    if !c.repo.cfg.notify_user {
        return None;
    }
    let turn = super::tally::take_receipt(&c.session, &c.repo.root, &c.log);
    match (late(&c, turn.is_some()), turn) {
        (Some(l), Some(t)) => Some(format!("{t}\n{l}")),
        (l, t) => t.or(l),
    }
}

/// PLAN-fael-local-first chunk 2: rows that have not reached their
/// destination (`sync::late_line`). `tracked`: only on a turn that filed or
/// closed — that is when a row can be left uncommitted, and quiet turns skip
/// the git spawn. `local`: once per session per distinct line, since a
/// failed sync or a repo with no way to share stays so across turns.
fn late(c: &super::protocol::Ctx, filed: bool) -> Option<String> {
    let tracked = matches!(c.repo.cfg.store, core::Store::Tracked);
    if tracked && !filed {
        return None;
    }
    let l = crate::sync::late_line(&c.repo, &c.log)?;
    (tracked || super::tally::first(&c.session, &c.repo.root, "late", &l)).then_some(l)
}

/// A sub-agent's stop only files its own reply's lines: the bug rule and
/// the once-per-session sync belong to the session's stop. No
/// transcript fallback — `session` is the parent's transcript, never this
/// agent's reply.
fn subagent_stop(e: &Event) -> Reply {
    if let (Some(c), Some(reply)) = (ctx(e), &e.reply)
        && adopted(&c)
    {
        super::cited::note_reply(&c, reply);
        capture::collect(&c, reply);
    }
    Reply::default()
}

/// A log under `.fael/` or in the clone's journal — without one fael was never
/// adopted here. The journal counts: `store = "local"` keeps rows nowhere else,
/// and a session or sub-agent in a fresh worktree has no `.fael/` of its own.
fn adopted(c: &super::protocol::Ctx) -> bool {
    crate::journal::home(&c.repo).is_some()
}

/// File the reply's capture lines; a bug announcement with no issue row since
/// is stashed for the next push. Never blocks — fael never starts a turn.
fn decide(e: &Event) -> Reply {
    let no = Reply::default();
    let Some(mut c) = ctx(e) else { return no };
    // no log in the tree or the journal = fael never adopted here — skip the
    // transcript read
    if !adopted(&c) {
        return no;
    }
    // the reply's lines are filed first, so an issue they write clears the
    // bug signal below
    if collect_reply(e, &c).stored > 0 {
        (c.log, c.tags) = crate::journal::read(&c.repo);
    }
    // session start: an RFC 3339 time, or a transcript file's birthtime, at
    // ms precision — whole seconds race with rows filed just before it
    let Some(since_ms) = session_start(e) else {
        return no;
    };
    if let Some(marker) = bug_marker(e, &c.log, since_ms, &c.repo.cfg) {
        stash_risk(&c.session, &c.repo.root, &marker);
    }
    no
}

/// File the last message's capture lines. The message comes from the client
/// (`reply`), else from the Claude transcript's tail; neither = nothing to do.
fn collect_reply(e: &Event, c: &super::protocol::Ctx) -> capture::Filed {
    let from_file = || {
        let t = Path::new(e.session.as_deref()?);
        t.is_file().then(|| capture::transcript_reply(t)).flatten()
    };
    match e.reply.clone().or_else(from_file) {
        Some(reply) => {
            super::cited::note_reply(c, &reply);
            capture::collect(c, &reply)
        }
        None => capture::Filed::default(),
    }
}

/// The turn's bug announcement, if any — free text, or the transcript tail
/// after the latest user message — unless an issue row at or after the match
/// already clears it. An issue filed before the words never does.
/// Phrases come from the repo's `[lang] marker` packs (PLAN-fael-languages).
fn bug_marker(e: &Event, log: &core::Log, since_ms: i64, cfg: &core::Config) -> Option<String> {
    let (marker, at_ms) = match (&e.text, e.session.as_deref()) {
        (Some(text), _) => (has_bug_marker(text, cfg)?, since_ms),
        (None, Some(t)) if Path::new(t).is_file() => {
            bug_signal_from_transcript(Path::new(t), since_ms, cfg)?
        }
        _ => return None,
    };
    let cleared = log
        .rows
        .iter()
        .any(|r| r.kind == "issue" && core::ts_ms(&r.ts).is_some_and(|ms| ms >= at_ms));
    (!cleared).then_some(marker)
}

/// The session start in ms: an RFC 3339 time (neutral callers without a
/// transcript), or a transcript file's birthtime (fallback: mtime). `None` =
/// no usable session.
fn session_start(e: &Event) -> Option<i64> {
    let s = e.session.as_deref()?;
    core::ts_ms(s).or_else(|| file_birth_ms(Path::new(s)).map(|m| m as i64))
}

/// Stash a risk line for the next push in this session — shown once,
/// then deleted. Empty session = no stash (no push would ever show it).
fn stash_risk(session: &str, worktree: &Path, marker: &str) {
    if session.is_empty() {
        return;
    }
    let path = risk_path(session, worktree);
    if path
        .parent()
        .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
    {
        let _ = std::fs::write(&path, format!("{marker}\n"));
    }
}

/// True when this session already marked this worktree + kind — else record
/// the mark and return false. Empty session = no dedupe. Auto sync's
/// once-per-newest-row guard; the `stop-block` dir name predates it.
pub(super) fn seen_before(session: &str, worktree: &str, kind: &str) -> bool {
    if session.is_empty() {
        return false;
    }
    let path = state_dir()
        .join("stop-block")
        .join(format!("{}.jsonl", session_key(session)));
    if let Ok(s) = std::fs::read_to_string(&path) {
        for line in s.lines() {
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue, // a torn line must not lose the rest
            };
            if v["worktree"] == *worktree && v["kind"] == *kind {
                return true;
            }
        }
    }
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        use std::io::Write;
        let row = serde_json::json!({
            "ts": now_rfc3339().unwrap_or_default(),
            "worktree": worktree, "kind": kind,
        });
        let mut f = match std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
        {
            Ok(f) => f,
            Err(_) => return false,
        };
        // seal a torn tail so the new row starts on its own line
        let seal = core::needs_seal(&mut f).unwrap_or(false);
        let _ = writeln!(f, "{}{row}", if seal { "\n" } else { "" });
    }
    false
}
