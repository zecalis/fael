//! Missing `verb`/`uid` over MCP — split from `chunk.rs` (file-size ratchet).

use super::args::{repo_for, s};
use super::chunk::KNOWN;
use serde_json::Value;

/// Unit-test entry: no live-run lookup, so the contract pins without a repo
/// or a `fael-crew` binary.
#[cfg(test)]
pub(super) fn shape_missing(a: &Value) -> String {
    shape_missing_with(a, None)
}

/// The reject `chunk_checked` returns: `sent` echoes the caller's keys (KNOWN
/// order, `cwd` is routing, not the shape); when this cwd holds one live run
/// its uid fills the retype, never a placeholder.
pub(super) fn shape_missing_with_live(a: &Value) -> String {
    shape_missing_with(a, live_uid_for(a))
}

fn shape_missing_with(a: &Value, live: Option<String>) -> String {
    let verb_opt = s(a, "verb");
    let uid_opt = s(a, "uid");
    let what = match (verb_opt.is_none(), uid_opt.is_none()) {
        (true, true) => "verb + uid",
        (true, false) => "verb",
        _ => "uid",
    };
    let mut sent: Vec<&str> = Vec::new();
    for k in KNOWN {
        let present = a
            .get(*k)
            .is_some_and(|v| !v.is_null() && v.as_str().is_none_or(|t| !t.is_empty()));
        if present {
            sent.push(*k);
        }
    }
    let sent_str = match sent.is_empty() {
        true => "none".to_string(),
        false => sent.join(", "),
    };
    let mut missing: Vec<&str> = Vec::new();
    if verb_opt.is_none() {
        missing.push("verb");
    }
    if uid_opt.is_none() {
        missing.push("uid");
    }
    let missing_str = missing.join(", ");
    let verb_show = verb_opt.as_deref().unwrap_or("start");
    let uid_show = uid_opt.as_deref().or(live.as_deref()).unwrap_or("<uid>");
    format!(
        "rejected: {what} is required for chunk — verb: show|status|start|note|done|wait|after; sent: {sent_str} · missing: {missing_str}; try chunk {{verb: \"{verb_show}\", uid: \"{uid_show}\", cwd: \"<your working directory>\"}} (cwd only on start)"
    )
}

/// The uid of the one live run in this call's repo, if exactly one — the fact
/// `fael chunk status` (no uid) already reads. `None` when the repo, the db or
/// the run is missing, or when two run here (ambiguous, never a guess).
fn live_uid_for(a: &Value) -> Option<String> {
    if s(a, "uid").is_some() {
        return None;
    }
    let r = repo_for(a).ok()?;
    let out =
        crate::crew::capture(&["chunk".into(), "status".into(), "--json".into()], &r.root).ok()?;
    let v: Value = serde_json::from_str(&out).ok()?;
    let arr = v.as_array()?;
    if arr.len() != 1 {
        return None;
    }
    arr[0].get("uid")?.as_str().map(String::from)
}
