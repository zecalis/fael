//! `chunk` arg contract: what MCP forwards, what it notes, never rejects.

use super::{KNOWN, build_args, misplaced_note, unknown_note, unknown_verb};

#[test]
fn an_unknown_verb_names_the_cli_when_it_is_one_there() {
    let e = unknown_verb("accept", "U1");
    assert!(e.contains("fael chunk accept U1"), "{e}");
    let e = unknown_verb("zzz", "U1");
    assert!(e.contains("try chunk {verb: \"show\""), "{e}");
    assert!(e.starts_with("rejected: unknown chunk verb zzz"), "{e}");
    // the closed list names `status` now that MCP serves it
    assert!(e.contains("show|status|start|note|done|wait|after"), "{e}");
}

#[test]
fn status_builds_like_show_read_only() {
    let a = json!({"verb": "status", "uid": "U1"});
    assert_eq!(
        build_args("status", "U1", &a).unwrap(),
        vec!["chunk", "status", "U1"]
    );
    // read-only: no run, no seat, no full
    for extra in [json!({"run": "R1"}), json!({"full": true})] {
        let mut b = a.clone();
        b.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let args = build_args("status", "U1", &b).unwrap();
        assert!(!args.contains(&"--run".to_string()), "{args:?}");
        assert!(!args.contains(&"--seat".to_string()), "{args:?}");
        assert!(!args.contains(&"--full".to_string()), "{args:?}");
    }
    // `full` rides only on start/show
    assert!(
        misplaced_note(
            &json!({"verb": "status", "uid": "U1", "full": true}),
            "status"
        )
        .unwrap()
        .contains("`full`")
    );
}
use serde_json::json;

#[test]
fn start_without_cwd_takes_the_servers_seat_and_says_it() {
    let (a, seat) = super::with_start_seat(&json!({"verb": "start", "uid": "U1"}));
    let cwd = a["cwd"].as_str().unwrap();
    assert!(!cwd.is_empty(), "{a}");
    let note = seat.unwrap();
    assert!(note.starts_with("seat: ") && note.contains(cwd), "{note}");
    // an explicit cwd, a launcher run, or another verb rides untouched
    for untouched in [
        json!({"verb": "start", "uid": "U1", "cwd": "/w"}),
        json!({"verb": "start", "uid": "U1", "run": "R1"}),
        json!({"verb": "done", "uid": "U1"}),
    ] {
        let (b, seat) = super::with_start_seat(&untouched);
        assert_eq!(b, untouched);
        assert!(seat.is_none(), "{untouched:?}");
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
fn elsewhere_machine_line_routes_and_old_prose_still_falls_back() {
    // fael-crew's machine line carries the root: MCP echoes it back as cwd.
    let e = "rejected: chunk 01M5J6M1 lives in /w/other — cd /w/other && fael chunk show 01M5J6M1\nfael-elsewhere cwd=/w/other".to_string();
    let got = super::with_cwd_hint(e.clone());
    assert!(got.starts_with(&e), "{got}");
    assert!(
        got.contains("MCP: re-run this call with \"cwd\": \"/w/other\""),
        "{got}"
    );
    // old binary without the machine line: the prose match still routes.
    let old =
        "rejected: chunk 01M5J6M1 lives in /w/other — cd /w/other && fael chunk show 01M5J6M1"
            .to_string();
    let got = super::with_cwd_hint(old);
    assert!(
        got.contains("MCP: re-run this call with \"cwd\" set to that repo root"),
        "{got}"
    );
    // unrelated errors pass through untouched.
    assert_eq!(
        super::with_cwd_hint("rejected: no chunk X".into()),
        "rejected: no chunk X"
    );
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
fn start_forwards_force_with_because() {
    let a =
        json!({"verb": "start", "uid": "U1", "force": true, "because": "seat dead, will avoid x"});
    let args = build_args("start", "U1", &a).unwrap();
    assert!(args.contains(&"--force".to_string()), "{args:?}");
    assert!(args.contains(&"--because".to_string()), "{args:?}");
    assert!(
        unknown_note(&a, KNOWN).is_none(),
        "because is schema, not hidden"
    );
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

#[test]
fn done_without_left_names_the_mcp_key_not_the_flag() {
    let e = "rejected: chunk done needs --left none or --left <id|uid>,… — how\ntry: fael chunk done U1 \"h\" --left none|<ids>".to_string();
    let got = super::with_cwd_hint(e);
    let line = got
        .lines()
        .find(|l| l.contains("left: \"none\"") && l.contains("left: \"<id|uid>\""))
        .expect(&got);
    assert!(!line.contains("--left"), "{got}");
    assert!(line.contains("U1"), "{got}");
}

#[test]
fn start_without_because_names_force_and_because_as_mcp_keys() {
    let e = "rejected: --force needs --because \"<what you will avoid>\" — try `fael chunk start U1 --force --because \"<what you will avoid>\"`".to_string();
    let got = super::with_cwd_hint(e);
    assert!(got.contains("force: true, because: \""), "{got}");
    assert!(got.contains("uid: \"U1\""), "{got}");
    let line = got
        .lines()
        .find(|l| l.contains("force: true, because: \""))
        .expect(&got);
    assert!(
        !line.contains("--force") && !line.contains("--because"),
        "{got}"
    );
}
