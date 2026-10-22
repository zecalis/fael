//! The stop event. It notes the ids the reply cites (`cited`), and stashes a fix or bug announcement with no issue or close after it for
//! the next push to show once. It never blocks: fael never starts an agent turn.

use super::markers::{has_bug_marker, has_fix_phrase, signal_from_transcript, transcript_reply};
use super::protocol::{Event, Reply, ctx};
use super::state::{
    file_birth_ms, fixed_path, now_rfc3339, risk_path, seen_path, session_key, state_dir,
};
use crate::core;
use std::path::Path;

/// The stop event: `decide`, then the once-per-session sync and the receipt.
pub(crate) fn stop(e: &Event) -> Reply {
    if e.agent.is_some() {
        return subagent_stop(e);
    }
    let mut r = decide(e);
    super::autosync::start(e);
    seen(e);
    r.notice = receipt(e);
    r
}

/// PLAN-fael-board: stamp the live chunk run of this worktree (SPEC §7) — `session` on the
/// first stop (or session start), `last_seen` on each. No crew.db, no open; any failure
/// stays silent.
pub(super) fn seen(e: &Event) {
    let Some(session) = e.session.as_deref().filter(|s| !s.is_empty()) else {
        return;
    };
    // the id `chunk start` read from $CLAUDE_CODE_SESSION_ID, not the transcript path
    let session = super::state::session_id(session);
    seen_held(session);
    let Some(r) = e
        .cwd
        .as_deref()
        .and_then(|c| crate::repo_at(Path::new(c)).ok())
    else {
        return;
    };
    let now = core::rfc3339(core::now_ms());
    let db = core::work::db_for(r.journal.as_deref(), &r.fael);
    let _ = core::work::mark_seen(&db, &r.root.to_string_lossy(), session, &now);
}

/// The run this session's `chunk start` bound, wherever the session works now: a
/// `--no-worktree` run has no worktree for [`seen`] to match (issue 01M4N4CVC). The marker
/// goes once no live run is left.
fn seen_held(session: &str) {
    let Some(m) = core::work::held_session_file(session) else {
        return;
    };
    // a db that is gone is never created again by the open
    let Some(db) = std::fs::read_to_string(&m)
        .ok()
        .filter(|d| Path::new(d).exists())
    else {
        let _ = std::fs::remove_file(m);
        return;
    };
    let now = core::rfc3339(core::now_ms());
    let n = core::work::mark_seen_session(Path::new(&db), session, &now);
    if n == 0 {
        let _ = std::fs::remove_file(m);
    }
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
    super::tally::take_receipt(&c.session, &c.repo.root, &c.log)
}

/// A sub-agent's stop only notes the ids its own reply cites: the bug rule
/// belongs to the session's stop. No
/// transcript fallback — `session` is the parent's transcript, never this
/// agent's reply.
fn subagent_stop(e: &Event) -> Reply {
    if let (Some(c), Some(reply)) = (ctx(e), &e.reply)
        && adopted(&c)
    {
        super::cited::note_reply(&c, reply);
    }
    Reply::default()
}

/// A log under `.fael/` or in the clone's journal — without one fael was never
/// adopted here. The journal counts: rows live nowhere else, and a session or sub-agent in a fresh worktree has no `.fael/` of its own.
fn adopted(c: &super::protocol::Ctx) -> bool {
    crate::journal::home(&c.repo).is_some()
}

/// Note the reply's cited ids; a fix or bug announcement with no issue
/// or close since is stashed for the next push — the fix phrase first, its
/// line asks for the whole cause → fix. Never blocks — fael never starts a turn.
fn decide(e: &Event) -> Reply {
    let no = Reply::default();
    let stop_ms = core::now_ms() as i64;
    let Some(c) = ctx(e) else { return no };
    // no log in the tree or the journal = fael never adopted here — skip the
    // transcript read
    if !adopted(&c) {
        return no;
    }
    note_reply(e, &c);
    // session start: an RFC 3339 time, or a transcript file's birthtime, at
    // ms precision — whole seconds race with rows filed just before it
    let Some(since_ms) = session_start(e) else {
        return no;
    };
    let cfg = &c.repo.cfg;
    // the honest order is fix → close → reply: a close anywhere in this turn
    // already filed what the reply says was fixed (fael:01M4F4CD)
    let fixed = signal(e, &c.log, (since_ms, stop_ms), true, &|t| {
        has_fix_phrase(t, cfg)
    });
    if fixed.is_some_and(|p| stash_fixed(&c.session, &c.repo.root, &p)) {
        return no;
    }
    if let Some(marker) = signal(e, &c.log, (since_ms, stop_ms), false, &|t| {
        has_bug_marker(t, cfg)
    }) {
        stash(&c.session, &risk_path(&c.session, &c.repo.root), &marker);
    }
    no
}

/// Note the ids the last message cites. The message comes from the client
/// (`reply`), else from the Claude transcript's tail; neither = nothing to do.
fn note_reply(e: &Event, c: &super::protocol::Ctx) {
    let from_file = || {
        let t = Path::new(e.session.as_deref()?);
        t.is_file().then(|| transcript_reply(t)).flatten()
    };
    if let Some(reply) = e.reply.clone().or_else(from_file) {
        super::cited::note_reply(c, &reply);
    }
}

/// The turn's announcement `hit` finds (a bug marker, a fix phrase), if any —
/// free text, or the transcript tail after the latest user message — unless
/// an issue or a close at or after the match already clears it. One filed
/// before the words never does. Phrases come from the repo's `[lang] marker`
/// packs (PLAN-fael-languages). Claude may fire Stop before the final reply
/// reaches the transcript, so the client's copy of it (`reply`) is read when
/// the file has no match. `(since_ms, stop_ms)`: session start, this stop.
/// `close_in_turn`: a close since the turn's prompt clears it too, even one
/// made before the words.
fn signal(
    e: &Event,
    log: &core::Log,
    (since_ms, stop_ms): (i64, i64),
    close_in_turn: bool,
    hit: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let (marker, at_ms, turn_ms) = match &e.text {
        Some(text) => (hit(text)?, since_ms, since_ms),
        None => {
            let (found, turn_ms) = e
                .session
                .as_deref()
                .map(Path::new)
                .filter(|t| t.is_file())
                .map_or((None, since_ms), |t| {
                    signal_from_transcript(t, since_ms, hit)
                });
            match found {
                Some((marker, at_ms)) => (marker, at_ms, turn_ms),
                None => (hit(e.reply.as_deref()?)?, stop_ms, turn_ms),
            }
        }
    };
    let at = |from: i64| move |r: &core::Row| core::ts_ms(&r.ts).is_some_and(|ms| ms >= from);
    let close_from = if close_in_turn { turn_ms } else { at_ms };
    let cleared = log.rows.iter().any(|r| r.kind == "issue" && at(at_ms)(r))
        || log.closes.iter().any(at(close_from));
    (!cleared).then_some(marker)
}

/// The session start in ms: an RFC 3339 time (neutral callers without a
/// transcript), or a transcript file's birthtime (fallback: mtime). `None` =
/// no usable session.
fn session_start(e: &Event) -> Option<i64> {
    let s = e.session.as_deref()?;
    core::ts_ms(s).or_else(|| file_birth_ms(Path::new(s)).map(|m| m as i64))
}

/// Stash a fix phrase for the next push, unless this session's fix line was
/// already said (`~fixed`, once per session) — false then, so the bug marker
/// may still be stashed. Empty session = no stash.
fn stash_fixed(session: &str, worktree: &Path, phrase: &str) -> bool {
    let seen = std::fs::read_to_string(seen_path(session, "", worktree)).unwrap_or_default();
    if seen.lines().any(|l| l == super::say::FIXED_KEY) {
        return false;
    }
    stash(session, &fixed_path(session, worktree), phrase)
}

/// Stash a phrase at `path`, a session file the next push in this session
/// shows once, then deletes. Empty session = no stash (no push would ever
/// show it) — false.
fn stash(session: &str, path: &Path, marker: &str) -> bool {
    if !session.is_empty()
        && path
            .parent()
            .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
    {
        return std::fs::write(path, format!("{marker}\n")).is_ok();
    }
    false
}

fn marks_path(session: &str) -> std::path::PathBuf {
    state_dir()
        .join("sync-seen")
        .join(format!("{}.jsonl", session_key(session)))
}

/// True when this session already marked this worktree + kind (read only).
pub(super) fn marked(session: &str, worktree: &str, kind: &str) -> bool {
    if session.is_empty() {
        return false;
    }
    let Ok(s) = std::fs::read_to_string(marks_path(session)) else {
        return false;
    };
    s.lines().any(|line| {
        // a torn line must not lose the rest
        serde_json::from_str::<serde_json::Value>(line)
            .is_ok_and(|v| v["worktree"] == *worktree && v["kind"] == *kind)
    })
}

/// True when this session already marked this worktree + kind — else record
/// the mark and return false. Empty session = no dedupe. Auto sync's
/// once-per-newest-row guard.
pub(super) fn seen_before(session: &str, worktree: &str, kind: &str) -> bool {
    if session.is_empty() {
        return false;
    }
    if marked(session, worktree, kind) {
        return true;
    }
    let path = marks_path(session);
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
