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
    "verb", "uid", "text", "other", "on", "run", "client", "model", "session", "force", "pr",
    "left", "review", "with", "as",
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
    if o.get("text").and_then(Value::as_str).is_none_or(str::is_empty)
        && let Some(v) = moved.into_iter().next()
    {
        o.insert("text".into(), v);
    }
    a
}

fn chunk_checked(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    let verb = need(a, "verb")?;
    let uid = need(a, "uid")?;
    let args = build_args(&verb, &uid, a)?;
    match crate::crew::capture(&args, &r.root) {
        Ok(t) => Ok(t),
        Err(e) if verb == "show" => show_elsewhere(&r, &uid, e),
        Err(e) => Err(with_cwd_hint(e)),
    }
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
        }
        "wait" => {
            need_text("the question")?;
            args.push(text.clone());
            args.extend(["--on".into(), s(a, "on").unwrap_or_else(|| "owner".into())]);
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
    Ok(args)
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
            for k in ["other", "on", "pr", "left", "review", "with", "as"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "start" => {
            if has_text {
                fell.push("`text` (start takes none)".into());
            }
            for k in ["other", "on", "pr", "left", "review"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "note" => {
            if has_run {
                fell.push("`run` (note takes none)".into());
            }
            for k in ["other", "on", "pr", "left", "review", "with", "as"] {
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
            for k in ["other", "pr", "left", "review", "with", "as"] {
                if has(k) {
                    fell.push(format!("`{k}` (only its verb takes it)"));
                }
            }
        }
        "after" => {
            for k in ["on", "pr", "left", "review", "with", "as"] {
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
mod tests {
    use super::{KNOWN, build_args, misplaced_note, unknown_note, unknown_verb};

    #[test]
    fn an_unknown_verb_names_the_cli_when_it_is_one_there() {
        let e = unknown_verb("accept", "U1");
        assert!(e.contains("fael chunk accept U1"), "{e}");
        let e = unknown_verb("zzz", "U1");
        assert!(e.contains("try chunk {verb: \"show\""), "{e}");
        assert!(e.starts_with("rejected: unknown chunk verb zzz"), "{e}");
    }
    use serde_json::json;

    #[test]
    fn start_forwards_client_and_model() {
        let a = json!({"verb": "start", "uid": "U1", "client": "opencode", "model": "deepseek-v1-flash"});
        let args = build_args("start", "U1", &a).unwrap();
        assert!(args.contains(&"--client".to_string()), "{args:?}");
        assert!(args.contains(&"opencode".to_string()), "{args:?}");
        assert!(args.contains(&"--model".to_string()), "{args:?}");
        assert!(args.contains(&"deepseek-v1-flash".to_string()), "{args:?}");
    }

    #[test]
    fn start_without_identity_forwards_nothing_new() {
        let a = json!({"verb": "start", "uid": "U1"});
        let args = build_args("start", "U1", &a).unwrap();
        assert_eq!(args, vec!["chunk", "start", "U1"], "{args:?}");
    }

    #[test]
    fn start_session_only_when_native_and_no_run() {
        // native id without launcher rides through
        let a = json!({"verb": "start", "uid": "U1", "session": "ses_abc123"});
        let args = build_args("start", "U1", &a).unwrap();
        assert!(args.contains(&"--session".to_string()), "{args:?}");
        // an RFC3339 hook session never rides (seen() writes last_seen-only)
        let a = json!({"verb": "start", "uid": "U1", "session": "2026-10-24T00:00:00.000Z"});
        let args = build_args("start", "U1", &a).unwrap();
        assert!(!args.contains(&"--session".to_string()), "{args:?}");
        // a launcher run never takes a session
        let a = json!({"verb": "start", "uid": "U1", "run": "R1", "session": "ses_abc123"});
        let args = build_args("start", "U1", &a).unwrap();
        assert!(args.contains(&"R1".to_string()), "{args:?}");
        assert!(!args.contains(&"--session".to_string()), "{args:?}");
    }

    #[test]
    fn start_forwards_joint_with_and_as() {
        let a = json!({"verb": "start", "uid": "U1", "with": "U2", "as": "lane"});
        let args = build_args("start", "U1", &a).unwrap();
        assert!(args.contains(&"--with".to_string()), "{args:?}");
        assert!(args.contains(&"U2".to_string()), "{args:?}");
        assert!(args.contains(&"--as".to_string()), "{args:?}");
        assert!(args.contains(&"lane".to_string()), "{args:?}");
        // joint keys are known now, and misplaced off start
        assert!(unknown_note(&a, KNOWN).is_none());
        let d = json!({"verb": "done", "uid": "U1", "text": "h", "with": "U2", "as": "lane"});
        let note = misplaced_note(&d, "done").unwrap();
        assert!(note.contains("`with`") && note.contains("`as`"), "{note}");
    }

    #[test]
    fn result_and_out_fill_text_and_the_fence_reject_speaks_mcp() {
        let a = json!({"verb": "done", "uid": "U1", "result": "r", "handoff": "o"});
        let a = super::text_alias(&a);
        assert_eq!(a["text"], "r");
        assert!(unknown_note(&a, KNOWN).is_none());
        let e = "rejected: x — try the same command with --run R1; else".to_string();
        assert!(super::with_cwd_hint(e).contains("same call with run: R1"));
    }

    #[test]
    fn unknown_keys_stay_notes_never_rejects() {
        // a future client field still builds — the note says it fell away
        let a = json!({"verb": "start", "uid": "U1", "future": "x"});
        assert!(build_args("start", "U1", &a).is_ok());
        let note = unknown_note(&a, KNOWN).unwrap();
        assert!(note.contains("future"), "{note}");
        // client/model/session are known now, so no unknown note for them
        let a = json!({"verb": "start", "uid": "U1", "client": "c", "model": "m", "session": "s"});
        assert!(unknown_note(&a, KNOWN).is_none());
        // but on done they fell off start — said, not dropped
        let a = json!({"verb": "done", "uid": "U1", "text": "h", "client": "c", "model": "m"});
        let note = misplaced_note(&a, "done").unwrap();
        assert!(
            note.contains("`client`") && note.contains("only start"),
            "{note}"
        );
    }
}
