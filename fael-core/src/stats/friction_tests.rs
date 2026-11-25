//! Friction tests: streams, first-call window, shape gate, top stream.

use super::super::parse::parse;
use std::path::{Path, PathBuf};

/// `(minute, cmd, outcome, empty, sig)` calls in one session → friction.
fn run(calls: &[(u32, &str, &str, bool, &str)]) -> super::Friction {
    let text: String = calls
        .iter()
        .map(|(m, cmd, outcome, empty, sig)| {
            format!(
                "{{\"ts\":\"2026-10-07T00:{m:02}:00.000Z\",\"repo\":\"/work/r\",\"client\":\"cli\",\"event\":\"call\",\"session\":\"s\",\"cmd\":\"{cmd}\",\"outcome\":\"{outcome}\",\"reason\":\"unknown_flag\",\"empty\":{empty},\"sig\":\"{sig}\"}}\n"
            )
        })
        .collect();
    let p = parse(
        &text,
        Path::new("/work/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    assert_eq!(p.n, 0, "call lines are no injections");
    super::friction(&p)
}

/// One call line; `session` empty = the line carries none.
#[allow(clippy::too_many_arguments)]
fn line(
    m: u32,
    session: &str,
    client: &str,
    cmd: &str,
    outcome: &str,
    empty: bool,
    sig: &str,
) -> String {
    let session = if session.is_empty() {
        String::new()
    } else {
        format!("\"session\":\"{session}\",")
    };
    format!(
        "{{\"ts\":\"2026-10-07T00:{m:02}:00.000Z\",\"repo\":\"/work/r\",\"client\":\"{client}\",\"event\":\"call\",{session}\"cmd\":\"{cmd}\",\"outcome\":\"{outcome}\",\"empty\":{empty},\"sig\":\"{sig}\"}}\n"
    )
}

#[test]
fn last_7d_counts_only_calls_within_a_week_of_now() {
    let l = [
        line(0, "s", "cli", "find", "ok", false, "a"),
        line(1, "s", "cli", "add", "reject", false, "b"),
    ];
    let p = parse(
        &l.concat(),
        Path::new("/work/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    let day = 86_400_000;
    let t = crate::ts_ms("2026-10-07T00:00:00.000Z").unwrap();
    let near = super::friction_at(&p, t + 6 * day).last_7d;
    assert_eq!((near.calls, near.rejects), (2, 1));
    let far = super::friction_at(&p, t + 8 * day);
    assert_eq!((far.last_7d.calls, far.total.calls), (0, 2));
}

fn of(lines: &[String]) -> super::Friction {
    let p = parse(
        &lines.concat(),
        Path::new("/work/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    super::friction(&p)
}

#[test]
fn sessions_do_not_mix_their_calls() {
    // s1's empty find, then s2's find, interleaved: not a repeat of s1's
    let f = of(&[
        line(0, "s1", "cli", "find", "ok", true, "a"),
        line(1, "s2", "cli", "find", "ok", false, "b"),
        line(2, "s1", "cli", "add", "ok", false, "c"),
    ]);
    assert_eq!(
        (f.total.find_repeat, f.total.first_call_ok),
        (0, 3),
        "{f:?}"
    );
}

#[test]
fn a_line_without_a_session_streams_by_repo_and_client() {
    let f = of(&[
        line(0, "", "mcp", "find", "ok", true, "a"),
        line(1, "", "mcp", "find", "ok", false, "b"),
    ]);
    assert_eq!(f.total.find_repeat, 1, "{f:?}");
    // another client's find is another agent
    let f = of(&[
        line(0, "", "mcp", "find", "ok", true, "a"),
        line(1, "", "cli", "find", "ok", false, "b"),
    ]);
    assert_eq!(f.total.find_repeat, 0, "{f:?}");
}

#[test]
fn friction_splits_by_client() {
    let f = of(&[
        line(0, "a", "opencode", "find", "reject", false, "a"),
        line(1, "b", "claude", "find", "ok", false, "b"),
        line(2, "b", "claude", "add", "ok", false, "c"),
    ]);
    assert_eq!(f.by_client["opencode"].rejects, 1, "{f:?}");
    assert_eq!(f.by_client["claude"].calls, 2, "{f:?}");
}

#[test]
fn calls_are_read_in_time_order_not_file_order() {
    // written out of order: the empty find is the earlier call
    let f = of(&[
        line(1, "s", "cli", "find", "ok", false, "b"),
        line(0, "s", "cli", "find", "ok", true, "a"),
    ]);
    assert_eq!(f.total.find_repeat, 1, "{f:?}");
}

#[test]
fn calls_at_the_tail_of_a_stream_are_judged_on_the_calls_that_exist() {
    // one call after, then the stream ends: both clean, both succeed
    let f = of(&[
        line(0, "s", "cli", "add", "ok", false, "a"),
        line(1, "s", "cli", "add", "ok", false, "b"),
    ]);
    assert_eq!(f.total.first_call_ok, 2, "{f:?}");
}

#[test]
fn a_lone_clean_call_is_a_first_call_success() {
    let f = run(&[(0, "add", "ok", false, "a")]);
    assert_eq!((f.total.calls, f.total.first_call_ok), (1, 1));
    assert_eq!(f.total.rejects + f.total.help + f.total.find_repeat, 0);
}

#[test]
fn a_reject_is_friction_and_spoils_the_call_before_it() {
    let f = run(&[
        (0, "add", "ok", false, "a"),
        (1, "find", "reject", false, "b"),
    ]);
    assert_eq!((f.total.rejects, f.total.first_call_ok), (1, 0));
    assert_eq!(f.total.reasons["unknown_flag"], 1);
    assert_eq!(f.by_command["find"].reasons["unknown_flag"], 1);
    assert_eq!(f.by_command["add"].first_call_ok, 0);
}

#[test]
fn a_shape_gate_reject_is_counted_apart_and_spoils_nothing() {
    let gate = line(1, "s", "cli", "add", "reject", false, "b").replace(
        "\"reject\"",
        "\"reject\",\"reason\":\"shape_gate\",\"rules\":[\"untitled\",\"no_key\"]",
    );
    let f = of(&[
        line(0, "s", "cli", "find", "ok", false, "a"),
        gate,
        line(2, "s", "cli", "add", "ok", false, "c"),
    ]);
    assert_eq!(f.shape_gate, 1);
    assert_eq!(f.shape_gate_rules["untitled"], 1, "{f:?}");
    assert_eq!((f.total.calls, f.total.rejects), (2, 0));
    assert_eq!(f.total.first_call_ok, 2, "{f:?}");
    assert!(f.total.reasons.is_empty(), "{f:?}");
}

#[test]
fn a_verb_gets_its_own_line() {
    let start = line(0, "s", "cli", "chunk", "reject", false, "a").replace(
        "\"reject\"",
        "\"reject\",\"reason\":\"chunk_state\",\"sub\":\"start\"",
    );
    let f = of(&[start, line(1, "s", "cli", "chunk", "ok", false, "b")]);
    assert_eq!(
        f.by_command["chunk start"].reasons["chunk_state"], 1,
        "{f:?}"
    );
    assert_eq!(f.by_command["chunk"].calls, 1, "{f:?}");
}

#[test]
fn a_reject_beyond_the_window_does_not_spoil() {
    let f = run(&[
        (0, "add", "ok", false, "a"),
        (1, "add", "ok", false, "b"),
        (2, "add", "ok", false, "c"),
        (3, "add", "ok", false, "d"),
        (4, "find", "help", false, "e"),
    ]);
    assert_eq!((f.total.help, f.total.first_call_ok), (1, 2));
}

#[test]
fn find_after_empty_find_is_a_repeat_but_refining_a_hit_is_not() {
    let f = run(&[(0, "find", "ok", true, "a"), (1, "find", "ok", false, "b")]);
    assert_eq!((f.total.find_repeat, f.total.first_call_ok), (1, 0));
    let f = run(&[(0, "find", "ok", false, "a"), (1, "find", "ok", false, "b")]);
    assert_eq!((f.total.find_repeat, f.total.first_call_ok), (0, 2));
    // a reject or help after an empty find is counted once, as itself
    let f = run(&[
        (0, "find", "ok", true, "a"),
        (1, "find", "help", false, "b"),
    ]);
    assert_eq!((f.total.find_repeat, f.total.help), (0, 1));
    // the same empty call again is a re-poll, not a fix
    let f = run(&[(0, "find", "ok", true, "a"), (1, "find", "ok", true, "a")]);
    assert_eq!(f.total.find_repeat, 0);
}

/// One looping session carries most rejects: `top_stream` holds its counts,
/// and `without` gives the rate of every other session (issue 01M88QVV).
#[test]
fn the_top_stream_is_the_session_with_most_friction() {
    let mut l: Vec<String> = (0..6)
        .map(|m| line(m, "loop", "claude", "chunk", "reject", false, "x"))
        .collect();
    l.push(line(10, "calm", "codex", "find", "ok", false, "a"));
    l.push(line(11, "calm", "codex", "add", "reject", false, "b"));
    l.push(line(12, "calm", "codex", "close", "ok", false, "c"));
    let f = of(&l);
    assert_eq!((f.top_stream.calls, f.top_stream.rejects), (6, 6));
    let rest = f.total.without(&f.top_stream);
    assert_eq!((rest.calls, rest.rejects, rest.bad()), (3, 1, 1));
    assert_eq!(rest.reasons.values().sum::<usize>(), 1);
    assert_eq!(
        super::Friction::default().top_stream.bad(),
        0,
        "no call, no top"
    );
}
