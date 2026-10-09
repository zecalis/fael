//! The prompt hint (01M3WCK7N): a user prompt that names an open key by one of
//! its segments, exactly, gets one pointer line for the agent — never rows
//! (push only on intent, 01M3SQ8AT), never a fuzzy match (`core::key_hints`).
//! Each key is pointed at once per session, and rows this session already
//! pushed or found (its `.seen` list) count too (01M3XKB7M): the agent does
//! not need a pointer at a key it just read rows from.

use super::asks::{UsageMeta, hook_meta};
use super::protocol::{Event, Reply, ctx};
use super::say::{Kind, Line, Outbox};
use super::state::{lock_seen, new_turn, seen_path, session_key, state_dir};
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
    // every prompt starts a turn, hint or not: the edit ask speaks once per turn
    new_turn(&c.session, &c.repo.root);
    let hints = core::key_hints(&c.log, &typed(text), &c.repo.cfg.hint_stop);
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
            &UsageMeta {
                said: r.said(),
                ..hook_meta(&c, None, false)
            },
        );
    }
    r
}

/// Blocks the harness puts in a prompt that the user never typed (01M4F5K3):
/// a background agent's finish notice, a reminder. Their words ("usage",
/// "background") named keys nobody asked about.
const HARNESS: [&str; 2] = ["task-notification", "system-reminder"];

/// The prompt as the user wrote it: harness blocks cut whole, and the markup
/// left (`<pasted_content id=…>`, whose "content" named a key) cut to a space —
/// the pasted text inside stays, the user put it there.
/// ponytail: any `<word…>` is cut as markup, so `a<b c>d` loses "b c" too
fn typed(text: &str) -> String {
    let mut s = text.to_string();
    for tag in HARNESS {
        let close = format!("</{tag}>");
        while let Some(i) = s.find(&format!("<{tag}")) {
            let end = s[i..].find(&close).map_or(s.len(), |j| i + j + close.len());
            s.replace_range(i..end, " ");
        }
    }
    let (mut out, mut rest) = (String::with_capacity(s.len()), s.as_str());
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let tag = after
            .trim_start_matches('/')
            .starts_with(|c: char| c.is_ascii_alphabetic());
        match after.find('>') {
            Some(j) if tag => {
                out.push(' ');
                rest = &after[j + 1..];
            }
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
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

#[cfg(test)]
mod tests {
    use super::typed;

    #[test]
    fn harness_text_is_not_the_prompt() {
        let note = "<task-notification>Agent finished, <usage>9</usage> background</task-notification> fix credit";
        assert_eq!(
            typed(note).split_whitespace().collect::<Vec<_>>(),
            ["fix", "credit"]
        );
        // a block never closed runs to the end
        assert_eq!(typed("a <system-reminder>usage").trim(), "a");
        // the paste stays, its markup goes
        let paste = "<pasted_content id=\"9\">credit rows</pasted_content id=\"9\"> why?";
        assert_eq!(
            typed(paste).split_whitespace().collect::<Vec<_>>(),
            ["credit", "rows", "why?"]
        );
        // a bare `<` is text
        assert_eq!(typed("a < b"), "a < b");
    }
}
