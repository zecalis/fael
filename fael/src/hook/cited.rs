//! The `cited` outcome (SPEC-fael-learn-loop §B): an id from the session's
//! seen list typed into a tool input or the closing reply. Written as a 0-byte
//! `outcome` usage line, never a push; each id once per session (a `^<id>`
//! mark in the seen list). The seen list also holds rows the agent filed or
//! found itself, so stats keeps only the ids a push said (or a pull showed
//! after a cut) — this side only finds the candidates.
//!
//! A `fael …` command is not a cite: `fael find <id>` is a pull, and stats
//! reads it from its own `found` line.

use super::asks::{UsageMeta, append_row};
use super::protocol::{Ctx, Event};
use super::state::{lock_seen, seen_path};
use super::usage::usage_row;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

/// Shortest id prefix that counts, as the push prints it (`[01M45R3J]`).
const SHORT: usize = 8;
const ID_LEN: usize = 26;

/// A tool call's input as text, minus the shell segments that run `fael`.
pub(crate) fn haystack(input: &Value) -> String {
    let mut v = input.clone();
    if let Some(cmd) = input["command"].as_str() {
        v["command"] = without_fael(cmd).into();
    }
    v.to_string()
}

fn without_fael(cmd: &str) -> String {
    cmd.split(['\n', ';', '&', '|'])
        .filter(|seg| {
            // `FAEL_STATE_DIR=x fael find …`: skip the env words
            let w = seg.split_whitespace().find(|w| !w.contains('='));
            !w.is_some_and(|w| w == "fael" || w.ends_with("/fael"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A tool event: the repo is resolved here, since a search with no files never
/// reaches `push`.
pub(crate) fn note_tool(e: &Event, input: &Value) {
    if e.session.as_deref().is_none_or(str::is_empty) || input.is_null() {
        return;
    }
    let cwd = e.cwd.as_deref().map_or_else(
        || std::env::current_dir().unwrap_or_default(),
        std::path::PathBuf::from,
    );
    let Ok(repo) = crate::repo_at(&cwd) else {
        return;
    };
    let session = e.session.as_deref().unwrap_or("");
    let agent = e.agent.as_deref().unwrap_or("");
    let client = e.client.as_deref().unwrap_or("neutral");
    note(client, session, agent, &repo.root, &haystack(input));
}

/// The closing reply, with the stop event's context already resolved.
pub(crate) fn note_reply(c: &Ctx, reply: &str) {
    note(&c.client, &c.session, &c.agent, &c.repo.root, reply);
}

fn note(client: &str, session: &str, agent: &str, root: &Path, text: &str) {
    if session.is_empty() {
        return;
    }
    let path = seen_path(session, agent, root);
    // the common case — nothing cited — costs one unlocked read
    let Ok(seen) = std::fs::read_to_string(&path) else {
        return;
    };
    if candidates(&seen, text).is_empty() {
        return;
    }
    let Some(mut f) = lock_seen(&path) else {
        return;
    };
    // re-read under the lock: a parallel hook may have marked them first
    let mut now = String::new();
    let _ = std::io::Read::read_to_string(&mut f, &mut now);
    let ids = candidates(&now, text);
    if ids.is_empty() {
        return;
    }
    let marks: String = ids.iter().map(|i| format!("^{i}\n")).collect();
    let _ = f.write_all(marks.as_bytes());
    let meta = UsageMeta {
        session: Some(session),
        agent: (!agent.is_empty()).then_some(agent),
        ..UsageMeta::default()
    };
    let mut row = usage_row(client, "outcome", root, "", &[], &meta);
    row["cited"] = ids.into();
    append_row(row);
}

/// Row ids in the seen list the text names, that no `^<id>` mark covers yet:
/// a full id cites itself; a short prefix cites only when no other seen id
/// shares it — batch-filed siblings share the timestamp prefix, so a shared
/// prefix is ambiguous, never a cite.
fn candidates(seen: &str, text: &str) -> Vec<String> {
    let marked = |id: &str| seen.lines().any(|l| l.strip_prefix('^') == Some(id));
    let mut ids: Vec<&str> = vec![];
    for id in seen.lines() {
        let id_like = id.len() == ID_LEN && id.bytes().all(|b| b.is_ascii_alphanumeric());
        if id_like && !marked(id) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut out: Vec<String> = vec![];
    for (i, id) in ids.iter().enumerate() {
        let named = text.contains(*id)
            || (text.contains(&id[..SHORT])
                && ids
                    .iter()
                    .enumerate()
                    .all(|(j, o)| j == i || o[..SHORT] != id[..SHORT]));
        if named {
            out.push(id.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_fael_command_is_no_cite_but_a_commit_message_is() {
        let t = |c: &str| haystack(&json!({"command": c}));
        assert!(!t("fael find 01M45R3J0A8Q39CQEH7YBCPA1N").contains("01M45R3J"));
        assert!(!t("cd x && FAEL_STATE_DIR=y /usr/bin/fael close 01M45R3J").contains("01M45R3J"));
        assert!(t("git commit -m 'fix per 01M45R3J'").contains("01M45R3J"));
        // a bare `fael` word later in a quoted message is no command
        assert!(t("echo 'x' && git commit -m \"a fael 01M45R3J\"").contains("01M45R3J"));
    }

    #[test]
    fn an_id_is_a_candidate_once_and_only_when_seen() {
        let id = "01M45R3J0A8Q39CQEH7YBCPA1N";
        let seen = format!("{id}\nk:x\n");
        assert_eq!(candidates(&seen, "see 01M45R3J here"), [id]);
        assert!(candidates(&seen, "see 01M45R3K here").is_empty());
        assert!(candidates(&seen, "see 01M45R3 here").is_empty(), "7 chars");
        assert!(
            candidates("k:x\n", "see 01M45R3J here").is_empty(),
            "never said"
        );
        assert!(candidates(&format!("{seen}^{id}\n"), id).is_empty(), "once");
    }

    #[test]
    fn same_ms_siblings_cite_only_the_named_full_id() {
        let (a, b) = (
            crate::core::ulid_at(1_789_000_000_000),
            crate::core::ulid_at(1_789_000_000_000),
        );
        assert_eq!(&a[..SHORT], &b[..SHORT], "one batch shares the prefix");
        let seen = format!("{a}\n{b}\n");
        let text = format!("see {a} here");
        let short = format!("see {} here", &a[..SHORT]);
        assert_eq!(candidates(&seen, &text), [a]);
        assert!(
            candidates(&seen, &short).is_empty(),
            "a shared prefix is ambiguous"
        );
    }
}
