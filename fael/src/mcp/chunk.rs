//! The MCP `chunk` tool — the CLI `chunk` verbs agents need without
//! switching screens: show, start, note, done, wait, after. One tool with a
//! `verb`, so the served schema stays small. The verbs run in `fael-crew`
//! (`crew::capture`): the same call, the same text.

use super::args::{need, repo_for, s};
use crate::Repo;
use serde_json::Value;

/// `chunk {"verb": ..., "uid": ..., "text": ...}`.
pub(super) fn chunk(a: &Value) -> Result<String, String> {
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

/// A `show` for a chunk this repo does not hold: read it where it lives (one
/// owner only — an ambiguous prefix stays a reject). Writes never follow.
fn show_elsewhere(r: &Repo, uid: &str, e: String) -> Result<String, String> {
    match crate::find::owner::chunk_elsewhere(r, uid) {
        Some(at) => {
            let args = vec!["chunk".into(), "show".into(), uid.to_string()];
            match crate::crew::capture(&args, &at) {
                Ok(t) => Ok(format!(
                    "in {} (MCP cwd \"{}\")\n{t}",
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
