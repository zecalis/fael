//! The notes `chunk` says about keys a verb ignores — split from `chunk.rs` (file-size ratchet).

use serde_json::Value;

/// A verb's keys that do nothing there — the opencode `pr`/`left` round-trip
/// (2026-10-22) was this: a `done` naming them silently dropped them. Say the
/// key and where it applies, never reject a call that saves.
pub(super) fn misplaced_note(a: &Value, verb: &str) -> Option<String> {
    let has = |k: &str| a.get(k).is_some_and(|v| !v.is_null());
    let filled = |k: &str| {
        a.get(k)
            .and_then(Value::as_str)
            .is_some_and(|t| !t.is_empty())
    };
    let has_force = a.get("force").and_then(Value::as_bool).unwrap_or(false);
    // (keys the verb takes none of, keys only another verb takes)
    let (none, foreign): (&[&str], &[&str]) = match verb {
        "show" => (
            &["text", "run"],
            &[
                "other",
                "on",
                "pr",
                "left",
                "review",
                "no_change",
                "with",
                "as",
            ],
        ),
        "start" => (
            &["text"],
            &["other", "on", "pr", "left", "review", "no_change"],
        ),
        "note" => (
            &["run"],
            &[
                "other",
                "on",
                "pr",
                "left",
                "review",
                "no_change",
                "with",
                "as",
            ],
        ),
        "done" => (&[], &["other", "on", "with", "as"]),
        "wait" => (
            &[],
            &["other", "pr", "left", "review", "no_change", "with", "as"],
        ),
        "after" => (
            &[],
            &["on", "pr", "left", "review", "no_change", "with", "as"],
        ),
        _ => return None,
    };
    let mut fell: Vec<String> = vec![];
    fell.extend(
        none.iter()
            .filter(|k| filled(k))
            .map(|k| format!("`{k}` ({verb} takes none)")),
    );
    fell.extend(
        foreign
            .iter()
            .filter(|k| has(k))
            .map(|k| format!("`{k}` (only its verb takes it)")),
    );
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
