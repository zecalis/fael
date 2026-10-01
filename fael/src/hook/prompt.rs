//! The prompt hint (01M3WCK7N): a user prompt that names an open key by one of
//! its segments, exactly, gets one pointer line for the agent — never rows
//! (push only on intent, 01M3SQ8AT), never a fuzzy match (`core::key_hints`).
//! Each key is pointed at once per session.

use super::asks::hook_meta;
use super::protocol::{Event, Reply, ctx};
use super::state::{lock_seen, session_key, state_dir};
use super::usage::record_usage;
use crate::core;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
    let Some(mut f) = lock_seen(&hinted_path(&c.session, &c.repo.root)) else {
        return Reply::default();
    };
    let mut done = String::new();
    let _ = f.read_to_string(&mut done);
    let new: Vec<_> = hints
        .iter()
        .filter(|k| !done.lines().any(|l| l == k.key))
        .collect();
    if new.is_empty() {
        return Reply::default();
    }
    let out: String = new.iter().map(|k| format!("{}\n", k.key)).collect();
    let _ = f.write_all(out.as_bytes());
    let list: Vec<String> = new
        .iter()
        .map(|k| format!("{} ({})", k.key, k.count))
        .collect();
    let line = format!(
        "fael: the prompt names open key(s) {} — fael find --key <key> (MCP: find key=<key>) before assuming there is no prior work",
        list.join(", ")
    );
    record_usage(
        &c.client,
        "prompt",
        &c.repo.root,
        &line,
        &[],
        &hook_meta(&c, None, false),
    );
    Reply {
        context: Some(line),
        ..Reply::default()
    }
}
