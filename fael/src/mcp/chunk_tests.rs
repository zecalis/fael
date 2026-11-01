//! `chunk` arg contract: what MCP forwards, what it notes, never rejects.

use super::{KNOWN, build_args, misplaced_note, start_needs_cwd, unknown_note, unknown_verb};

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
fn start_without_cwd_is_rejected_unless_a_run_names_the_seat() {
    let e = start_needs_cwd(&json!({"verb": "start", "uid": "U1"}), "start", "U1").unwrap_err();
    assert!(
        e.starts_with("rejected: cwd is required") && e.contains("cwd: \""),
        "{e}"
    );
    for ok in [
        json!({"verb": "start", "uid": "U1", "cwd": "/w"}),
        json!({"verb": "start", "uid": "U1", "run": "R1"}),
        json!({"verb": "done", "uid": "U1"}),
    ] {
        let verb = ok["verb"].as_str().unwrap();
        assert!(start_needs_cwd(&ok, verb, "U1").is_ok(), "{ok}");
    }
}

#[test]
fn start_forwards_client_and_model() {
    let a =
        json!({"verb": "start", "uid": "U1", "client": "opencode", "model": "deepseek-v1-flash"});
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
    let e = "x — try the same command with --run R1; else".to_string();
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

#[test]
fn wait_forwards_until_and_options_as_the_cli_spells_them() {
    // c5: wait shape rides through MCP so the CLI shape-checks before record.
    let a = json!({"verb": "wait", "uid": "U1", "text": "q", "on": "data", "until": "2026-12-01", "options": ["A", "B"]});
    let args = build_args("wait", "U1", &a).unwrap();
    assert!(args.contains(&"--until".to_string()), "{args:?}");
    assert!(args.contains(&"2026-12-01".to_string()), "{args:?}");
    assert_eq!(
        args.iter().filter(|x| *x == "--option").count(),
        2,
        "{args:?}"
    );
    assert!(unknown_note(&a, KNOWN).is_none(), "wait shape is known");
    let a = json!({"verb": "wait", "uid": "U1", "text": "q", "on": "owner", "option": "A"});
    let args = build_args("wait", "U1", &a).unwrap();
    assert!(
        args.contains(&"--option".to_string()) && args.contains(&"A".to_string()),
        "{args:?}"
    );
}

#[test]
fn start_forwards_full_for_the_resumed_brief() {
    // c5: same-session resume prints short — `full: true` reprints the whole brief.
    let a = json!({"verb": "start", "uid": "U1", "full": true});
    let args = build_args("start", "U1", &a).unwrap();
    assert!(args.contains(&"--full".to_string()), "{args:?}");
    assert!(unknown_note(&a, KNOWN).is_none(), "full is known (hidden)");
    let w = json!({"verb": "wait", "uid": "U1", "text": "q", "full": true});
    assert!(
        misplaced_note(&w, "wait").unwrap().contains("`full`"),
        "{w}"
    );
}

#[test]
fn done_wait_after_without_cwd_or_run_ask_for_the_runs_seat() {
    for verb in ["done", "wait", "after"] {
        let a = json!({"verb": verb, "uid": "U1", "text": "t", "other": "U2"});
        let args = build_args(verb, "U1", &a).unwrap();
        assert!(args.contains(&"--seat".to_string()), "{verb}: {args:?}");
        for named in [json!({"cwd": "/w"}), json!({"run": "R1"})] {
            let mut b = a.clone();
            b.as_object_mut()
                .unwrap()
                .extend(named.as_object().unwrap().clone());
            let args = build_args(verb, "U1", &b).unwrap();
            assert!(!args.contains(&"--seat".to_string()), "{verb}: {args:?}");
        }
    }
    // start and note never look for a seat: start records one, note answers from anywhere
    for verb in ["start", "note"] {
        let a = json!({"verb": verb, "uid": "U1", "text": "t"});
        let args = build_args(verb, "U1", &a).unwrap();
        assert!(!args.contains(&"--seat".to_string()), "{verb}: {args:?}");
    }
}

#[test]
fn a_call_missing_verb_gets_the_whole_start_shape_once() {
    let e = super::shape_missing(&json!({"uid": "U1"}));
    assert!(
        e.contains("verb is required") && e.contains("cwd: \""),
        "{e}"
    );
    let e = super::shape_missing(&json!({}));
    assert!(e.contains("verb + uid is required"), "{e}");
}
