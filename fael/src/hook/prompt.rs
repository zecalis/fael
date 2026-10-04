//! The prompt hint (01M3WCK7N): a user prompt that names an open key by one of
//! its segments, exactly, gets one pointer line for the agent — never rows
//! (push only on intent, 01M3SQ8AT), never a fuzzy match (`core::key_hints`).
//! Each key is pointed at once per session, and rows this session already
//! pushed or found (its `.seen` list) count too (01M3XKB7M): the agent does
//! not need a pointer at a key it just read rows from.

use super::asks::hook_meta;
use super::protocol::{Event, Reply, ctx};
use super::say::{Kind, Line, Outbox};
use super::state::{lock_seen, seen_path, session_key, state_dir};
use super::usage::record_usage;
use crate::core;
use std::path::{Path, PathBuf};

/// The call the pointer offers.
const FIND: &str = "fael find --key <key>";

/// Keys this session was already pointed at, one per line.
fn hinted_path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.keys"))
}

/// `e.text` is the prompt. No session = no once-only list, so no hint.
pub(crate) fn prompt(e: &Event) -> Reply {
    let (Some(c), Some(text)) = (ctx(e), e.text.as_deref()) else {
        return Reply::default();
    };
    if c.session.is_empty() {
        return Reply::default();
    }
    let hints = core::key_hints(&c.log, text);
    if hints.is_empty() {
        return Reply::default();
    }
    let Some(f) = lock_seen(&hinted_path(&c.session, &c.repo.root)) else {
        return Reply::default();
    };
    let mut out = Outbox::open(Some(f));
    // also what this session already pushed or found (01M3XKB7M): the .seen
    // list holds those row ids — a key whose rows are all in it is already
    // in context, so pointing at it would repeat what the agent just read.
    // A key still pointing something new (any id not seen) stays eligible.
    let seen: String = read_seen(&c.session, &c.repo.root);
    let seen_ids: Vec<&str> = seen.lines().collect();
    let new: Vec<_> = hints
        .iter()
        .filter(|(k, _)| !out.has(&k.key) && !rows_all_seen(&c.log, &k.key, &seen_ids))
        .collect();
    if new.is_empty() {
        return Reply::default();
    }
    let list: Vec<String> = new
        .iter()
        .map(|(k, words)| format!("{} ({}, via \"{}\")", k.key, k.count, words.join(" ")))
        .collect();
    out.say(Line {
        kind: Kind::Pointer {
            keys: new.iter().map(|(k, _)| k.key.clone()).collect(),
        },
        text: format!(
            "fael: the prompt names open key(s) {} — {FIND} (MCP: find key=<key>) before assuming there is no prior work",
            list.join(", ")
        ),
    });
    let r = out.reply();
    // usage counts only a line that reached the agent
    if let Some(context) = r.context() {
        record_usage(
            &c.client,
            "prompt",
            &c.repo.root,
            context,
            &[],
            &hook_meta(&c, None, false),
        );
    }
    r
}

/// The session's seen list, empty when there is none.
fn read_seen(session: &str, root: &Path) -> String {
    std::fs::read_to_string(seen_path(session, "", root)).unwrap_or_default()
}

/// True when every row this session could carry from `key` is already in the
/// session's seen list — a pointer would say "read the thing you just read".
/// Only the key's *open* rows count (01M3XKB7E review): a superseded or
/// closed older version is never pushed, so counting it would defeat the skip
/// on any re-filed key — and `key_hints` only ever points at open keys.
fn rows_all_seen(log: &core::Log, key: &str, seen: &[&str]) -> bool {
    let (closed, gone) = (core::closed(log), core::superseded(log));
    let ids: Vec<&str> = log
        .rows
        .iter()
        .filter(|r| {
            r.key.as_deref() == Some(key)
                && !closed.contains(r.id.as_str())
                && !gone.contains(r.id.as_str())
                && !core::is_carrier_row(r)
                && !core::is_alias_row(r)
        })
        .map(|r| r.id.as_str())
        .collect();
    !ids.is_empty() && ids.iter().all(|id| seen.contains(id))
}
