//! The MCP `chunk` tool — the CLI `chunk` verbs agents need without
//! switching screens: show, start, note, done, wait, after. One tool with a
//! `verb`, so the served schema stays small. The verbs run in `fael-crew`
//! (`crew::capture`): the same call, the same text.

use super::args::{need, repo_for, s, unknown_note, with_notes};
use crate::Repo;
use serde_json::Value;

/// No hidden `chunk` properties — every verb key is in the schema; a key the
/// verb ignores is still known, so it gets the verb note below, not unknown.
#[allow(dead_code)] // test-only: documents which KNOWN entries are hidden from the schema
pub(crate) const HIDDEN: &[&str] = &[];
/// Every key `chunk` reads.
pub(crate) const KNOWN: &[&str] = &[
    "verb", "uid", "text", "other", "on", "run", "force", "pr", "left",
];

/// `chunk {"verb": ..., "uid": ..., "text": ...}`.
pub(super) fn chunk(a: &Value) -> Result<String, String> {
    let extra = unknown_note(a, KNOWN);
    let verb = s(a, "verb").unwrap_or_default();
    let misplaced = misplaced_note(a, &verb);
    let res = chunk_checked(a);
    with_notes(res, vec![extra, misplaced])
}

fn chunk_checked(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    let verb = need(a, "verb")?;
    let uid = need(a, "uid")?;
    let text = s(a, "text").unwrap_or_default();
    let need_text = |what: &str| match text.is_empty() {
        true => Err(format!(
            "rejected: text is required for chunk {verb} — {what}"
        )),
        false => Ok(()),
    };
    let mut args: Vec<String> = vec!["chunk".into(), verb.clone(), uid.clone()];
    match verb.as_str() {
        "show" | "start" => {}
        "note" => {
            if text.is_empty() {
                return Err("rejected: text is required for chunk note".into());
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
        }
        "wait" => {
            need_text("the question")?;
            args.push(text.clone());
            args.extend(["--on".into(), s(a, "on").unwrap_or_else(|| "owner".into())]);
        }
        "after" => {
            need_text("why")?;
            let other = s(a, "other")
                .ok_or("rejected: other is required for chunk after — the chunk waited on")?;
            args.extend([other, text.clone()]);
        }
        v => {
            return Err(format!(
                "rejected: unknown chunk verb {v} — show|start|note|done|wait|after"
            ));
        }
    }
    if let Some(run) = s(a, "run").filter(|_| verb != "show" && verb != "note") {
        args.extend(["--run".into(), run]);
    }
    if verb == "start" && a["force"].as_bool().unwrap_or(false) {
        args.push("--force".into());
    }
    match crate::crew::capture(&args, &r.root) {
        Ok(t) => Ok(t),
        Err(e) if verb == "show" => show_elsewhere(&r, &uid, e),
        Err(e) => Err(with_cwd_hint(e)),
    }
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

/// A verb's keys that do nothing there — the opencode `pr`/`left` round-trip
/// (2026-10-22) was this: a `done` naming them silently dropped them. Say the
/// key and where it applies, never reject a call that saves.
fn misplaced_note(a: &Value, verb: &str) -> Option<String> {
    let has = |k: &str| a.get(k).is_some_and(|v| !v.is_null());
    let has_text = a
        .get("text")
        .and_then(Value::as_str)
        .is_some_and(|t| !t.is_empty());
    let has_run = a
        .get("run")
        .and_then(Value::as_str)
        .is_some_and(|t| !t.is_empty());
    let has_force = a.get("force").and_then(Value::as_bool).unwrap_or(false);
    let mut fell: Vec<String> = vec![];
    match verb {
        "show" => {
            if has_text {
                fell.push("`text` (show takes none)".into());
            }
            if has_run {
                fell.push("`run` (show takes none)".into());
            }
            if has_force {
                fell.push("`force` (only start takes it)".into());
            }
            for k in ["other", "on", "pr", "left"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "start" => {
            if has_text {
                fell.push("`text` (start takes none)".into());
            }
            for k in ["other", "on", "pr", "left"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "note" => {
            if has_run {
                fell.push("`run` (note takes none)".into());
            }
            if has_force {
                fell.push("`force` (only start takes it)".into());
            }
            for k in ["other", "on", "pr", "left"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "done" => {
            for k in ["other", "on"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
            if has_force {
                fell.push("`force` (only start takes it)".into());
            }
        }
        "wait" => {
            for k in ["other", "pr", "left"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
            if has_force {
                fell.push("`force` (only start takes it)".into());
            }
        }
        "after" => {
            for k in ["on", "pr", "left"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
            if has_force {
                fell.push("`force` (only start takes it)".into());
            }
        }
        _ => return None,
    }
    match fell.as_slice() {
        [] => None,
        [one] => Some(format!("note: {one} ignored for chunk {verb}")),
        many => Some(format!(
            "note: {} ignored for chunk {verb}",
            many.join(", ")
        )),
    }
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
    match e.contains("lives in") {
        true => format!("{e}\nMCP: re-run this call with \"cwd\" set to that repo root"),
        false => e,
    }
}
