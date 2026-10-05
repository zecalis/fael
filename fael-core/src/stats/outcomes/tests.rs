use super::*;
use std::path::{Path, PathBuf};

/// A push-shaped usage line of session `s1` at minute `min`.
fn line(min: u8, rest: &str) -> String {
    format!(
        "{{\"ts\":\"2026-10-05T00:{min:02}:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"s1\",{rest}}}\n"
    )
}

fn run(usage: &str, closes: &str) -> BTreeMap<String, RowOutcomes> {
    let row = |id: &str| {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-04T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[\"a.rs\"]}}\n"
        )
    };
    let mut log = Log {
        rows: super::super::said::tests::rows(&["A", "G", "H", "I", "N"].map(row).concat()),
        ..Log::default()
    };
    let (bumps, closes): (Vec<_>, Vec<_>) = super::super::said::tests::rows(closes)
        .into_iter()
        .partition(|r| r.bumps.is_some());
    log.rows.extend(bumps);
    log.closes = closes;
    let p = super::super::parse::parse(
        usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    outcomes(&p, &HashMap::from([("/w/r".to_string(), log)]))
}

fn close(id: &str, ts: &str) -> String {
    format!(
        "{{\"v\":1,\"id\":\"C{id}\",\"ts\":\"{ts}\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"{id}\"}}\n"
    )
}

#[test]
fn each_outcome_is_observed_and_its_fail_examples_are_not() {
    let usage = line(
        0,
        r#""event":"search","ids":["A","N"],"said":[{"kind":"row","key":"A"},{"kind":"pointer","key":"k:x"},{"kind":"count","key":"a.rs|file"}],"cut":[{"id":"G","r":"gate"},{"id":"H","r":"gate"},{"id":"I","r":"cap"}]"#,
    ) + &line(1, r#""event":"outcome","ids":[],"cited":["A","Z"]"#)
        // a pointer key, then a by-id pull of a shown row
        + &line(2, r#""event":"find","found":["A"],"q":{"key":"k:x"}"#)
        + &line(3, r#""event":"find","found":["N"],"q":{"id":"N"}"#)
        // G pulled by the agent, H through the count line's call, I by the agent
        + &line(4, r#""event":"find","found":["G"],"q":{"id":"G"}"#)
        + &line(5, r#""event":"find","found":["H"],"q":{"files":["a.rs"]}"#)
        + &line(6, r#""event":"find","found":["I"],"q":{"id":"I"}"#)
        + &line(7, r#""event":"outcome","ids":[],"cited":["G","I"]"#);
    let o = run(&usage, &close("A", "2026-10-05T01:00:00Z"));
    let a = &o["A"];
    assert_eq!((a.shown, a.cited, a.acted), (1, 1, 1), "{a:?}");
    assert_eq!(
        a.pulled,
        Pulled {
            agent_initiated: 0,
            fael_induced: 1
        }
    );
    assert!(!o.contains_key("Z"), "an id fael never said is no cite");
    assert_eq!(
        o["N"].pulled.agent_initiated, 1,
        "a by-id pull of a shown row"
    );
    // gate cut, agent pulled it, then cited it
    assert_eq!((o["G"].retrieved_after_cut, o["G"].missed_push), (1, 1));
    assert_eq!(o["G"].cut["gate"], 1);
    // gate cut, pulled only through the count line fael said
    assert_eq!((o["H"].retrieved_after_cut, o["H"].missed_push), (0, 0));
    // cap cut: the agent came back and cited it, but no policy cut it
    assert_eq!((o["I"].retrieved_after_cut, o["I"].missed_push), (1, 0));
    assert_eq!(o["I"].cut["cap"], 1);
}

/// The shadow writer says the row and lists it in `would_drop` on one line:
/// the agent's own pull still reads as demand, a count-line pull does not.
#[test]
fn a_said_would_drop_row_pulled_by_the_agent_is_retrieved() {
    let usage = line(
        0,
        r#""event":"search","ids":["S","T"],"said":[{"kind":"count","key":"a.rs|file"}],"would_drop":{"policy":"touch@1","ids":["S","T"]}"#,
    ) + &line(1, r#""event":"find","found":["S"],"q":{"id":"S"}"#)
        + &line(2, r#""event":"find","found":["T"],"q":{"files":["a.rs"]}"#)
        + &line(3, r#""event":"outcome","ids":[],"cited":["S","T"]"#);
    let o = run(&usage, "");
    assert_eq!((o["S"].shown, o["S"].cut["would_drop"]), (1, 1));
    assert_eq!(o["S"].pulled.agent_initiated, 1);
    assert_eq!((o["S"].retrieved_after_cut, o["S"].missed_push), (1, 1));
    assert_eq!(o["T"].pulled.fael_induced, 1);
    assert_eq!((o["T"].retrieved_after_cut, o["T"].missed_push), (0, 0));
}

#[test]
fn a_pull_without_a_cite_or_act_is_no_missed_push() {
    let usage = line(0, r#""event":"search","ids":[],"cut":[{"id":"G","r":"gate"}]"#)
        + &line(1, r#""event":"find","found":["G"],"q":{"id":"G"}"#)
        + &line(2, r#""event":"outcome","ids":[],"cited":["G"]"#)
        // a second row: pulled, then closed within the day
        + &line(3, r#""event":"search","ids":[],"would_drop":{"policy":"touch@1","ids":["H"]},"cut":[]"#)
        + &line(4, r#""event":"find","found":["H"],"q":{"id":"H"}"#);
    let o = run(&usage, &close("H", "2026-10-05T02:00:00Z"));
    assert_eq!((o["G"].retrieved_after_cut, o["G"].missed_push), (1, 1));
    assert_eq!(o["H"].cut["would_drop"], 1);
    assert_eq!((o["H"].retrieved_after_cut, o["H"].missed_push), (1, 1));
    let usage = line(
        0,
        r#""event":"search","ids":[],"cut":[{"id":"G","r":"gate"}]"#,
    ) + &line(1, r#""event":"find","found":["G"],"q":{"id":"G"}"#);
    let o = run(&usage, "");
    assert_eq!((o["G"].retrieved_after_cut, o["G"].missed_push), (1, 0));
}

#[test]
fn sessions_count_once_each_and_the_day_window_holds() {
    let in_s2 = |min: u8, rest: &str| line(min, rest).replace("\"s1\"", "\"s2\"");
    let push = r#""event":"read","ids":["A","G"]"#;
    // two sessions, each told A twice: two observations, not four
    let usage = line(0, push) + &line(1, push) + &in_s2(2, push) + &in_s2(3, push);
    // A closed 3 days after the first push: outside the window. G bumped inside it
    let bump = "{\"v\":1,\"id\":\"B\",\"ts\":\"2026-10-05T05:00:00Z\",\"by\":\"w\",\"kind\":\"bump\",\"text\":\"t\",\"files\":[],\"bumps\":\"G\"}\n";
    let o = run(&usage, &(close("A", "2026-10-08T00:00:00Z") + bump));
    assert_eq!((o["A"].shown, o["A"].acted), (2, 0), "{:?}", o["A"]);
    assert_eq!((o["G"].shown, o["G"].acted), (2, 2), "{:?}", o["G"]);
}

#[test]
fn an_outcome_line_is_no_injection() {
    let usage = line(0, r#""event":"read","bytes":9,"est_tokens":2,"ids":["A"]"#)
        + &line(
            1,
            r#""event":"outcome","bytes":0,"est_tokens":0,"ids":[],"cited":["A"]"#,
        );
    let p = super::super::parse::parse(
        &usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    assert_eq!((p.n, p.by_event.len()), (1, 1));
    assert!(p.rows.len() == 1 && p.kept.len() == 2);
}
