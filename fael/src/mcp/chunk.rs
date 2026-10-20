//! The MCP `chunk` tool — the CLI `chunk` verbs agents need without
//! switching screens: show, start, note, done, wait, after. One tool with a
//! `verb`, so the served schema stays small. The verbs run in `fael-crew`
//! (`crew::capture`): the same call, the same text.

use super::args::{need, repo_for, s};
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
    let mut args: Vec<String> = vec!["chunk".into(), verb.clone(), uid];
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
    crate::crew::capture(&args, &r.root)
}
