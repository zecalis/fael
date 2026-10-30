//! The MCP `chunk` tool — the CLI `chunk` verbs agents need without
//! switching screens: show, start, note, done, wait, after. One tool with a
//! `verb`, so the served schema stays small. The verbs run in `fael-crew`
//! (`crew::capture`): the same call, the same text.

use super::args::{need, repo_for, s, unknown_note, with_notes};
use super::chunk_misplaced::misplaced_note;
use crate::Repo;
use serde_json::Value;

/// `no_change` is born hidden (issue 01M62VY2; every schema byte is paid each session): the
/// no-diff line names it. `until`/`option` (wait shape) and `full` (resume short
/// unless --full, c5) ride hidden too: forwarded, never a schema byte, never an
/// unknown-arg note. Every other verb key is in the schema; a key the verb ignores is
/// still known, so it gets the verb note below, not unknown.
#[allow(dead_code)] // test-only: documents which KNOWN entries are hidden from the schema
pub(crate) const HIDDEN: &[&str] = &["no_change", "until", "option", "options", "full"];
/// Every key `chunk` reads.
pub(crate) const KNOWN: &[&str] = &[
    "verb",
    "uid",
    "text",
    "other",
    "on",
    "until",
    "option",
    "options",
    "full",
    "run",
    "client",
    "model",
    "session",
    "force",
    "pr",
    "left",
    "review",
    "no_change",
    "with",
    "as",
];

/// `chunk {"verb": ..., "uid": ..., "text": ...}`.
pub(super) fn chunk(a: &Value) -> Result<String, String> {
    let a = &text_alias(a);
    let extra = unknown_note(a, KNOWN);
    let verb = s(a, "verb").unwrap_or_default();
    let misplaced = misplaced_note(a, &verb);
    let res = chunk_checked(a);
    with_notes(res, vec![extra, misplaced])
}

/// The chunk rules say `result` / handoff, the schema says `text`: an alias
/// fills an empty `text` and is dropped, so it is never an unknown-arg note.
fn text_alias(a: &Value) -> Value {
    let mut a = a.clone();
    let Some(o) = a.as_object_mut() else { return a };
    let moved: Vec<Value> = ["result", "handoff"]
        .iter()
        .filter_map(|k| o.remove(*k))
        .collect();
    if o.get("text")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
        && let Some(v) = moved.into_iter().next()
    {
        o.insert("text".into(), v);
    }
    a
}

fn chunk_checked(a: &Value) -> Result<String, String> {
    let verb = need(a, "verb")?;
    let uid = need(a, "uid")?;
    start_needs_cwd(a, &verb, &uid)?;
    let r = repo_for(a)?;
    let args = build_args(&verb, &uid, a)?;
    match crate::crew::capture(&args, &r.root) {
        Ok(t) => Ok(t),
        Err(e) if verb == "show" => show_elsewhere(&r, &uid, e),
        Err(e) => Err(with_cwd_hint(e)),
    }
}

/// A start records the caller's cwd as its seat, and `done` later refuses a run started from
/// elsewhere: with no `cwd` the client's launch dir becomes the seat. Said at the start, not
/// at the done. A pinned server has one workspace; a launcher's `run` already has its seat.
fn start_needs_cwd(a: &Value, verb: &str, uid: &str) -> Result<(), String> {
    if verb != "start" || super::pinned() || s(a, "cwd").is_some() || s(a, "run").is_some() {
        return Ok(());
    }
    Err(format!(
        "rejected: cwd is required for chunk start — it records your seat, and chunk done refuses a run started from another directory; try chunk {{verb: \"start\", uid: \"{uid}\", cwd: \"<your working directory>\"}}"
    ))
}

/// The argv `chunk_checked` runs — pure, so unit tests pin the contract without
/// a repo or a `fael-crew` binary: unknown keys ride along (forward-compat
/// notes, never a reject here).
fn build_args(verb: &str, uid: &str, a: &Value) -> Result<Vec<String>, String> {
    let text = s(a, "text").unwrap_or_default();
    let need_text = |what: &str| match text.is_empty() {
        true => Err(format!(
            "rejected: text is required for chunk {verb} — {what}; try chunk {{verb: \"{verb}\", uid: \"{uid}\", text: \"<{what}>\"}}"
        )),
        false => Ok(()),
    };
    let mut args: Vec<String> = vec!["chunk".into(), verb.to_string(), uid.to_string()];
    match verb {
        "show" | "start" => {}
        "note" => {
            if text.is_empty() {
                return Err(format!(
                    "rejected: text is required for chunk note; try chunk {{verb: \"note\", uid: \"{uid}\", text: \"<what the next start needs>\"}}"
                ));
            }
            args.push(text.clone());
        }
        "done" => {
            need_text("the handoff")?;
            args.push(text.clone());
            if let Some(p) = pr_arg(a) {
                args.extend(["--pr".into(), p]);
            }
            if let Some(l) = left_arg(a) {
                args.extend(["--left".into(), l]);
            }
            if let Some(v) = s(a, "review") {
                args.extend(["--review".into(), v]);
            }
            if a["no_change"].as_bool().unwrap_or(false) {
                args.push("--no-change".into());
            }
        }
        "wait" => {
            need_text("the question")?;
            args.push(text.clone());
            args.extend(["--on".into(), s(a, "on").unwrap_or_else(|| "owner".into())]);
            // c5: wait shape rides through — `--until` and repeatable `--option`
            // forward as the CLI spells them; the CLI shape-checks before record.
            if let Some(u) = s(a, "until") {
                args.extend(["--until".into(), u]);
            }
            for o in option_args(a) {
                args.extend(["--option".into(), o]);
            }
        }
        "after" => {
            need_text("why")?;
            let other = s(a, "other")
                .ok_or(format!(
                    "rejected: other is required for chunk after — the chunk waited on; try chunk {{verb: \"after\", uid: \"{uid}\", other: \"<uid it waits on>\", text: \"<why>\"}}"
                ))?;
            args.extend([other, text.clone()]);
        }
        v => return Err(unknown_verb(v, uid)),
    }
    if let Some(run) = s(a, "run").filter(|_| verb != "show" && verb != "note") {
        args.extend(["--run".into(), run]);
    }
    if verb == "start" {
        // explicit wins (COALESCE explicit→approval→hint); forward what the agent said
        if let Some(c) = s(a, "client") {
            args.extend(["--client".into(), c]);
        }
        if let Some(m) = s(a, "model") {
            args.extend(["--model".into(), m]);
        }
        // no launcher (--run): the caller session binds the run, native ids only —
        // an RFC3339 hook session is never stored (seen() writes last_seen-only)
        if s(a, "run").is_none()
            && let Some(sess) = s(a, "session").filter(|v| fael_core::ts_ms(v).is_none())
        {
            args.extend(["--session".into(), sess]);
        }
        // joint start (01M5PE238): --with/--as ride the CLI contract as is
        if let Some(w) = s(a, "with") {
            args.extend(["--with".into(), w]);
        }
        if let Some(m) = s(a, "as") {
            args.extend(["--as".into(), m]);
        }
    }
    if verb == "start" && a["force"].as_bool().unwrap_or(false) {
        args.push("--force".into());
    }
    // c5: same-session resume prints short — `--full` reprints the whole brief.
    if verb == "start" && a["full"].as_bool().unwrap_or(false) {
        args.push("--full".into());
    }
    Ok(args)
}

/// `wait --option`: one string, or many — `option` plus `options[]`, comma-split
/// like the CLI's repeatable flag. Anything else rides through for the CLI to reject.
fn option_args(a: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(o) = a["option"].as_str() {
        out.push(o.to_string());
    }
    if let Some(arr) = a["option"].as_array() {
        out.extend(arr.iter().filter_map(Value::as_str).map(String::from));
    }
    if let Some(arr) = a["options"].as_array() {
        out.extend(arr.iter().filter_map(Value::as_str).map(String::from));
    }
    if let Some(o) = a["options"].as_str() {
        out.push(o.to_string());
    }
    out
}

/// `done --pr N`: a number or its string; anything else rides through for the
/// CLI to reject — MCP never invents a stricter gate than `chunk done` has.
fn pr_arg(a: &Value) -> Option<String> {
    a["pr"]
        .as_str()
        .map(String::from)
        .or_else(|| a["pr"].as_i64().map(|n| n.to_string()))
}

/// `done --left`: `none`, one id, or many — an array rides comma-joined, as
/// the CLI spells it. Anything else rides through for the CLI to reject.
fn left_arg(a: &Value) -> Option<String> {
    if let Some(s) = a["left"].as_str() {
        return Some(s.to_string());
    }
    a["left"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|s| !s.is_empty())
}

/// A `show` for a chunk this repo does not hold: read it where it lives (one
/// owner only — an ambiguous prefix stays a reject). Writes never follow.
fn show_elsewhere(r: &Repo, uid: &str, e: String) -> Result<String, String> {
    match crate::find::owner::chunk_elsewhere(r, uid) {
        Some(at) => {
            let args = vec!["chunk".into(), "show".into(), uid.to_string()];
            match crate::crew::capture(&args, &at) {
                Ok(t) => Ok(format!(
                    "in {} (chunk) — pass {{\"cwd\": \"{}\"}} to work there\n{t}",
                    at.display(),
                    at.display()
                )),
                Err(_) => Err(e),
            }
        }
        None => Err(with_cwd_hint(e)),
    }
}

/// A write naming a chunk in another repo keeps pointing there, now with the
/// MCP routing beside the CLI one — same call, plus `"cwd"`.
fn with_cwd_hint(e: String) -> String {
    if e.contains("same command with --run ") {
        return e.replace("same command with --run ", "same call with run: ");
    }
    match e.contains("lives in") {
        true => format!("{e}\nMCP: re-run this call with \"cwd\" set to that repo root"),
        false => e,
    }
}

/// An MCP verb outside `show|start|note|done|wait|after`: the closed list, and for a real
/// `fael chunk` verb the MCP lacks, the CLI line to run instead (a fact, never a guess).
fn unknown_verb(v: &str, uid: &str) -> String {
    let tail = if crate::core::work::CHUNK_VERBS.contains(&v) {
        format!("`{v}` is a CLI verb — run: fael chunk {v} {uid} (fael chunk --help)")
    } else {
        format!("try chunk {{verb: \"show\", uid: \"{uid}\"}}")
    };
    format!("rejected: unknown chunk verb {v} — show|start|note|done|wait|after; {tail}")
}

#[cfg(test)]
#[path = "chunk_tests.rs"]
mod tests;
