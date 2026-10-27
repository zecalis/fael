//! Yield tests apart from `said.rs`, to keep it under the file-size limit:
//! `finding` (PLAN-fael-experience-loop chunk 2) and the `count` line's call
//! (`outcomes` still reads old count lines).

use super::said::{Pull, counted, tests::rows, yields};
use crate::Log;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The ask split reads the hub flag the push recorded and earns like
/// `ask`; an ask with no `feat` counts in `ask` only.
#[test]
fn ask_splits_by_the_recorded_hub_flag() {
    let row = |id: &str| {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[\"a.rs\"]}}\n"
        )
    };
    let log = Log {
        rows: rows(&(row("H") + &row("F") + &row("X"))),
        closes: rows(
            "{\"v\":1,\"id\":\"C\",\"ts\":\"2026-09-26T00:08:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"H\"}\n",
        ),
        ..Log::default()
    };
    let usage = r#"{"ts":"2026-09-26T00:01:00.000Z","repo":"/w/r","client":"claude","session":"/t/s1.jsonl","event":"edit","ids":[],"said":[{"kind":"ask","key":"H"},{"kind":"ask","key":"F"},{"kind":"ask","key":"X"}],"feat":{"H":{"hub":true},"F":{"hub":false}}}
"#;
    let p = super::parse::parse(
        usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    let y = yields(&p, &HashMap::from([("/w/r".to_string(), log)]));
    let got = |k: &str| y.get(k).map(|y| (y.said, y.earned));
    assert_eq!(got("ask"), Some((3, 1)));
    assert_eq!(got("ask:hub"), Some((1, 1)), "H closed after the ask");
    assert_eq!(
        got("ask:file"),
        Some((1, 0)),
        "F never retired; X has no feat"
    );
}

/// A finding earns only on an issue naming its file, filed by the same
/// session after the line: not before it, not by another session, not on
/// another file, not a decision.
#[test]
fn a_finding_earns_only_on_the_sessions_later_issue_on_its_file() {
    let row = |id: &str, ts: &str, kind: &str, file: &str, session: &str| {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-09-26T{ts}Z\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"{file}\"],\"session\":\"{session}\"}}\n"
        )
    };
    let usage = |file: &str| {
        format!(
            "{{\"ts\":\"2026-09-26T00:05:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",\"event\":\"review\",\"ids\":[],\"said\":[{{\"kind\":\"finding\",\"key\":\"{file}\"}}]}}\n"
        )
    };
    let earned = |body: String, file: &str| {
        let log = Log {
            rows: rows(&body),
            ..Log::default()
        };
        let p = super::parse::parse(
            &usage(file),
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let y = yields(&p, &HashMap::from([("/w/r".to_string(), log)]));
        (y["finding"].said, y["finding"].earned)
    };
    let ok = row("A", "00:06:00", "issue", "a.rs", "s1");
    assert_eq!(earned(ok, "a.rs"), (1, 1));
    let cases = [
        (
            "before the line",
            row("A", "00:04:00", "issue", "a.rs", "s1"),
        ),
        (
            "another session",
            row("A", "00:06:00", "issue", "a.rs", "s2"),
        ),
        ("another file", row("A", "00:06:00", "issue", "b.rs", "s1")),
        ("a decision", row("A", "00:06:00", "decision", "a.rs", "s1")),
    ];
    for (why, body) in cases {
        assert_eq!(earned(body, "a.rs"), (1, 0), "{why}");
    }
}

#[test]
fn a_count_line_earns_on_the_call_it_printed() {
    let pull = |files: &[&'static str], key: Option<&'static str>| Pull {
        ms: 0,
        key,
        files: files.to_vec(),
        id: None,
    };
    let files = |p: &Pull| counted("src/a.rs,src/b.rs|file", p);
    assert!(files(&pull(&["src/b.rs"], None)), "a file it named");
    assert!(!files(&pull(&["lib/"], None)), "another directory");
    assert!(
        !files(&pull(&["src/a"], None)),
        "a prefix that is no directory"
    );
    assert!(!files(&pull(&[], Some("k:a"))), "a key pull");
    let dir = |p: &Pull| counted("src/a.rs|dir:src/", p);
    assert!(dir(&pull(&["src/"], None)), "`+N more in src/`");
    assert!(dir(&pull(&["src"], None)), "the same call, normalised");
    let key = |p: &Pull| counted("src/a.rs|key:auth:session", p);
    assert!(key(&pull(&[], Some("auth:session"))), "`+N more with #key`");
    assert!(!key(&pull(&[], Some("auth:other"))), "a key it never named");
    assert!(!key(&pull(&["src/a.rs"], None)), "a file pull");
    let keys = |p: &Pull| counted("src/a.rs|keys", p);
    assert!(keys(&pull(&[], Some("any:key"))), "`+N more under 3 keys`");
}

/// r2: `held.acted` counts stamped retire events in the window, per `shown`
/// source split close / supersede / bump. One entry per stamped event, not
/// per row — the fix for 01M57RES on the new count; the old `earned` rule
/// (earliest event within a day) is unchanged.
fn held_yields(usage: &str, rows_body: &str, closes_body: &str) -> super::said::KindYield {
    let (bumps, rest): (Vec<_>, Vec<_>) =
        rows(rows_body).into_iter().partition(|r| r.bumps.is_some());
    let mut log = Log {
        rows: rest,
        ..Log::default()
    };
    log.rows.extend(bumps);
    log.closes = rows(closes_body);
    let p = super::parse::parse(
        usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    yields(&p, &HashMap::from([("/w/r".to_string(), log)]))["held"].clone()
}

fn held_usage() -> String {
    "{\"ts\":\"2026-09-26T00:05:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",\"event\":\"edit\",\"ids\":[\"H\"],\"said\":[{\"kind\":\"held\",\"key\":\"H\"}]}\n".to_string()
}

#[test]
fn held_acted_counts_a_stamped_close_once() {
    let rows_body = "{\"v\":1,\"id\":\"H\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]}\n";
    let closes_body = "{\"v\":1,\"id\":\"C\",\"ts\":\"2026-09-26T00:06:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"done\",\"files\":[],\"ref\":\"H\",\"shown\":[\"held\"]}\n";
    let y = held_yields(&held_usage(), rows_body, closes_body);
    let a = y.acted.get("held").expect("held source");
    assert_eq!((a.close, a.supersede, a.bump), (1, 0, 0));
    assert_eq!((y.said, y.earned), (1, 1), "old earned rule unchanged");
}

#[test]
fn held_acted_ignores_an_unstamped_bump() {
    let rows_body = "{\"v\":1,\"id\":\"H\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]}\n{\"v\":1,\"id\":\"B1\",\"ts\":\"2026-09-26T00:06:00Z\",\"by\":\"w\",\"text\":\"H bumped\",\"bumps\":\"H\"}\n";
    let y = held_yields(&held_usage(), rows_body, "");
    assert!(
        y.acted.is_empty(),
        "no stamp = nowhere in acted: {:?}",
        y.acted
    );
    assert_eq!(y.earned, 1, "old earned still counts the unstamped bump");
}

#[test]
fn held_acted_counts_a_stamped_bump_and_close_both() {
    let rows_body = "{\"v\":1,\"id\":\"H\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]}\n{\"v\":1,\"id\":\"B1\",\"ts\":\"2026-09-26T00:06:00Z\",\"by\":\"w\",\"text\":\"H bumped\",\"bumps\":\"H\",\"shown\":[\"held\"]}\n";
    let closes_body = "{\"v\":1,\"id\":\"C\",\"ts\":\"2026-09-26T00:07:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"done\",\"files\":[],\"ref\":\"H\",\"shown\":[\"held\"]}\n";
    let y = held_yields(&held_usage(), rows_body, closes_body);
    let a = y.acted.get("held").expect("held source");
    assert_eq!((a.close, a.bump), (1, 1), "every stamped event counts");
    assert_eq!(y.earned, 1, "earned still counts the row once");
}

#[test]
fn held_acted_splits_supersede_and_other_sources() {
    let rows_body = "{\"v\":1,\"id\":\"H\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]}\n{\"v\":1,\"id\":\"S\",\"ts\":\"2026-09-26T00:06:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"],\"supersedes\":\"H\",\"shown\":[\"held\"]}\n{\"v\":1,\"id\":\"D\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]}\n";
    let closes_body = "{\"v\":1,\"id\":\"C\",\"ts\":\"2026-09-26T00:06:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"done\",\"files\":[],\"ref\":\"D\",\"shown\":[\"done\"]}\n{\"v\":1,\"id\":\"Cold\",\"ts\":\"2026-09-26T00:04:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"old\",\"files\":[],\"ref\":\"H\",\"shown\":[\"held\"]}\n";
    let y = held_yields(&held_usage(), rows_body, closes_body);
    assert_eq!(y.acted.get("held").map(|a| a.supersede), Some(1));
    assert_eq!(y.acted.get("done").map(|a| a.close), Some(1));
    assert_eq!(
        y.acted.get("held").map(|a| a.close),
        Some(0),
        "pre-window close never counts"
    );
}

/// `overlap` / `checkpoint` earn on a later `fael chunk note` call of the same
/// session (no ledger row exists for it): not before the line, not by another
/// session, not a rejected call.
#[test]
fn overlap_and_checkpoint_earn_on_a_later_chunk_note_call() {
    let call = |min: u8, session: &str, outcome: &str| {
        format!(
            "{{\"ts\":\"2026-09-26T00:0{min}:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"event\":\"call\",\"session\":\"{session}\",\"cmd\":\"chunk\",\"sub\":\"note\",\"outcome\":\"{outcome}\"}}\n"
        )
    };
    let said = "{\"ts\":\"2026-09-26T00:02:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",\"event\":\"edit\",\"ids\":[],\"said\":[{\"kind\":\"overlap\",\"key\":\"U\"},{\"kind\":\"checkpoint\",\"key\":\"~checkpoint:U:0\"}]}\n";
    let earned = |extra: String| {
        let p = super::parse::parse(
            &(said.to_string() + &extra),
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let y = yields(&p, &HashMap::new());
        (y["overlap"].earned, y["checkpoint"].earned)
    };
    assert_eq!(earned(call(3, "s1", "ok")), (1, 1));
    assert_eq!(earned(call(1, "s1", "ok")), (0, 0), "before the line");
    assert_eq!(earned(call(3, "s2", "ok")), (0, 0), "another session");
    assert_eq!(earned(call(3, "s1", "reject")), (0, 0), "rejected");
}

/// Issue 01M5ZH6V: `briefdrift` and `retired` entries were said but fell into
/// the tally's catch-all and vanished. Each now tallies, earns on its own act
/// (a chunk note / a decision filed after it), and an old `count` entry is
/// still skipped.
#[test]
fn briefdrift_and_retired_tally_and_earn() {
    let call = "{\"ts\":\"2026-09-26T00:03:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"event\":\"call\",\"session\":\"/t/s1.jsonl\",\"cmd\":\"chunk\",\"sub\":\"note\",\"outcome\":\"ok\"}\n";
    let said = "{\"ts\":\"2026-09-26T00:02:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s1.jsonl\",\"event\":\"edit\",\"ids\":[],\"said\":[{\"kind\":\"briefdrift\",\"key\":\"~drift:U@7\"},{\"kind\":\"retired\",\"key\":\"N\"},{\"kind\":\"count\",\"key\":\"x\"}]}\n";
    let decision = "{\"v\":1,\"id\":\"D\",\"ts\":\"2026-09-26T00:04:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"],\"session\":\"s1\"}\n";
    let run = |usage: String, body: &str| {
        let log = Log {
            rows: rows(body),
            ..Log::default()
        };
        let p = super::parse::parse(
            &usage,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let y = yields(&p, &HashMap::from([("/w/r".to_string(), log)]));
        assert!(!y.contains_key("count"), "an old name stays skipped");
        ["briefdrift", "retired"].map(|k| (y[k].said, y[k].earned))
    };
    assert_eq!(
        run(said.to_string(), ""),
        [(1, 0), (1, 0)],
        "said, not acted"
    );
    assert_eq!(run(format!("{said}{call}"), decision), [(1, 1), (1, 1)]);
}
