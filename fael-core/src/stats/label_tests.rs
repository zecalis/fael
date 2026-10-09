//! `label` (PLAN-fael-label chunk 3): the three states off one fixture log.

use super::label::{LABEL_SINCE, Label, Measure, State, label};
use super::said::tests::rows;
use crate::Log;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A row at `ts` (after the contract when it starts `10-1`).
fn row(id: &str, ts: &str, kind: &str, rest: &str) -> String {
    format!(
        "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-{ts}Z\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"a.rs\"]{rest}}}\n"
    )
}

fn close(id: &str, ts: &str, text: &str) -> String {
    format!(
        "{{\"v\":1,\"id\":\"X{id}{ts}\",\"ts\":\"2026-{ts}Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"{text}\",\"files\":[],\"ref\":\"{id}\"}}\n"
    )
}

/// A: core and guard · B: closed twice, the latest without core · C: closed
/// before the contract · D: its close folded in, core only · E: open. Keys:
/// K1 before the contract; K2 reuses it; K3 new; K4 supersedes K3 (left
/// out); K6 is filed after K5 but stamped before the contract, so by ts K5
/// is the reuse and K6 is not counted.
fn log() -> Log {
    let folded =
        r#","closed":{"id":"XD","ts":"2026-10-11T00:00:00Z","by":"w","text":"stale → reload"}"#;
    Log {
        rows: rows(
            &(row("A", "10-10T00:00:00", "issue", "")
                + &row("B", "10-10T00:00:00", "issue", "")
                + &row("C", "10-01T00:00:00", "issue", "")
                + &row("D", "10-10T00:00:00", "issue", folded)
                + &row("E", "10-10T00:00:00", "issue", "")
                + &row("K1", "10-01T00:00:00", "note", r#","key":"a:x""#)
                + &row("K2", "10-10T00:00:00", "note", r#","key":"a:x""#)
                + &row("K3", "10-10T00:00:00", "decision", r#","key":"b:y""#)
                + &row(
                    "K4",
                    "10-11T00:00:00",
                    "decision",
                    r#","key":"b:y","supersedes":"K3""#,
                )
                + &row("K5", "10-12T00:00:00", "note", r#","key":"c:z""#)
                + &row("K6", "10-01T00:00:00", "note", r#","key":"c:z""#)),
        ),
        closes: rows(
            &(close("A", "10-11T00:00:00", "loop → cap; guard `tests/a.rs`")
                + &close("B", "10-11T00:00:00", "cause → fix")
                + &close("B", "10-12T00:00:00", "reopened, fixed in a1b2c3d")
                + &close("C", "10-02T00:00:00", "cause → fix")),
        ),
        ..Log::default()
    }
}

/// `label` over usage at `(repo, day)`.
fn run(repos: &[(&str, &str)], logs: HashMap<String, Log>) -> Label {
    let usage: String = repos
        .iter()
        .map(|(r, day)| format!("{{\"ts\":\"2026-{day}T00:00:00.000Z\",\"repo\":\"{r}\",\"client\":\"claude\",\"event\":\"read\",\"ids\":[]}}\n"))
        .collect();
    let p = super::parse::parse(
        &usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    label(&p, &logs)
}

fn m(state: State, num: usize, den: usize) -> Measure {
    Measure {
        state,
        num,
        den,
        since: Some(LABEL_SINCE.into()),
    }
}

#[test]
fn measured_unmeasurable_and_gone_repos() {
    let one = HashMap::from([("/w/r".to_string(), log())]);
    let find_hit = Measure::default(); // never kept: no since either
    assert_eq!(
        run(&[("/w/r", "10-10")], one.clone()),
        Label {
            close_core: m(State::Measured, 2, 3), // A, D of A, B, D
            guard: m(State::Measured, 1, 3),      // A
            key_reuse: m(State::Measured, 2, 3),  // K2, K5 of K2, K3, K5
            find_hit: find_hit.clone(),
            gone_repos: 0,
        }
    );
    // two worktrees reading the same log: each row counts once
    let two = HashMap::from([("/w/r".to_string(), log()), ("/w/q".to_string(), log())]);
    let both = run(&[("/w/r", "10-10"), ("/w/q", "10-10")], two);
    assert_eq!(
        (both.close_core, both.key_reuse),
        (m(State::Measured, 2, 3), m(State::Measured, 2, 3))
    );
    // a removed worktree used since the contract: its rows count through the
    // live checkout sharing its journal, so the state holds; the path is counted
    let gone = run(&[("/w/r", "10-10"), ("/w/gone", "10-10")], one.clone());
    assert_eq!(gone.close_core, m(State::Measured, 2, 3));
    assert_eq!(gone.key_reuse, m(State::Measured, 2, 3));
    assert_eq!(gone.gone_repos, 1);
    // gone, but last used before the contract: it had nothing to count
    let before = run(&[("/w/r", "10-10"), ("/w/gone", "10-01")], one);
    assert_eq!(before.close_core, m(State::Measured, 2, 3));
    assert_eq!(before.gone_repos, 0);
    // nothing closed or keyed since the contract: no base, never 0%
    let empty = run(
        &[("/w/r", "10-10")],
        HashMap::from([("/w/r".into(), Log::default())]),
    );
    assert_eq!(
        empty,
        Label {
            close_core: m(State::Unmeasurable, 0, 0),
            guard: m(State::Unmeasurable, 0, 0),
            key_reuse: m(State::Unmeasurable, 0, 0),
            find_hit,
            gone_repos: 0,
        }
    );
}
