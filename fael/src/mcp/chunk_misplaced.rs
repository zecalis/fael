//! The notes `chunk` says about keys a verb ignores — split from `chunk.rs` (file-size ratchet).

use serde_json::Value;

/// A verb's keys that do nothing there — the opencode `pr`/`left` round-trip
/// (2026-10-22) was this: a `done` naming them silently dropped them. Say the
/// key and where it applies, never reject a call that saves.
pub(super) fn misplaced_note(a: &Value, verb: &str) -> Option<String> {
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
            for k in [
                "other",
                "on",
                "pr",
                "left",
                "review",
                "no_change",
                "with",
                "as",
            ] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "start" => {
            if has_text {
                fell.push("`text` (start takes none)".into());
            }
            for k in ["other", "on", "pr", "left", "review", "no_change"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "note" => {
            if has_run {
                fell.push("`run` (note takes none)".into());
            }
            for k in [
                "other",
                "on",
                "pr",
                "left",
                "review",
                "no_change",
                "with",
                "as",
            ] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "done" => {
            for k in ["other", "on", "with", "as"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "wait" => {
            for k in ["other", "pr", "left", "review", "no_change", "with", "as"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "after" => {
            for k in ["on", "pr", "left", "review", "no_change", "with", "as"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        _ => return None,
    }
    if verb != "start" {
        // `force`+identity+joint ride only on `start`
        if has_force {
            fell.push("`force` (only start takes it)".into());
        }
        for k in ["client", "model", "session", "with", "as"] {
            if has(k) {
                fell.push(format!("`{k}` (only start takes it)"));
            }
        }
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
