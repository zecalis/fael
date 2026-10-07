//! Per-rule self-heal precision from restore labels — against an in-memory
//! `Log` (`doctor_precision` judges rows, never files).
//!
//! Acceptance (PLAN-fael-selfheal-restore chunk 3): usable only with a label —
//! no labels, or a re-add alone, counts nothing; pre-verdict edges (no
//! `decision_source`) are skipped; an explicit re-supersede after the
//! restore overturns the label, an auto-healer one never does.

use super::common::row;
use fael_core::*;

const B: &str = "C0000000000000000000000020";
const A: &str = "C0000000000000000000000021";
const R: &str = "C0000000000000000000000022";
const E: &str = "C0000000000000000000000023";

fn edge(id: &str, target: &str, source: Option<&str>) -> Row {
    let mut r = row(id, "note", &["src/a.rs"]);
    r.key = Some("auth:session".into());
    r.supersedes = Some(target.into());
    r.decision_source = source.map(String::from);
    r
}

fn restore(id: &str, edge: &str, target: &str) -> Row {
    let mut r = Row::restored("tester-0000", edge, target);
    r.id = id.into();
    r
}

fn log(rows: Vec<Row>) -> Log {
    Log {
        rows,
        ..Log::default()
    }
}

#[test]
fn no_labels_not_counted() {
    let l = log(vec![
        row(B, "note", &["src/a.rs"]),
        edge(A, B, Some("identity:key")),
    ]);
    assert!(doctor_precision(&l).is_none());
}

#[test]
fn readd_alone_not_counted() {
    // the healer files a new edge over the open row — a prediction, no label
    let l = log(vec![
        row(B, "note", &["src/a.rs"]),
        edge(A, B, Some("identity:key")),
        edge(E, A, Some("heuristic:files")),
    ]);
    assert!(doctor_precision(&l).is_none());
}

#[test]
fn unknown_source_edges_skipped() {
    // a restore labels the edge, but its rule predates verdict — still nothing
    let l = log(vec![
        row(B, "note", &["src/a.rs"]),
        edge(A, B, None),
        restore(R, A, B),
    ]);
    assert!(doctor_precision(&l).is_none());
}

#[test]
fn restore_labels_wrong_per_rule() {
    let l = log(vec![
        row(B, "note", &["src/a.rs"]),
        edge(A, B, Some("identity:key")),
        restore(R, A, B),
        edge(
            "C0000000000000000000000024",
            "C0000000000000000000000025",
            Some("heuristic:files"),
        ),
    ]);
    let p = doctor_precision(&l).expect("one label lands");
    assert_eq!(p.kind, ProblemKind::Precision);
    assert_eq!(p.severity, Severity::Info);
    assert!(!p.fixable);
    assert!(
        p.detail
            .contains("identity:key 0/1 correct, 1 restored of 1 edges"),
        "{}",
        p.detail
    );
    assert!(
        p.detail.contains("1 unlabeled edge(s) not counted"),
        "{}",
        p.detail
    );
    assert_eq!(p.ids, vec![A.to_string()]);
}

#[test]
fn explicit_reaffirm_counts_right() {
    let l = log(vec![
        row(B, "note", &["src/a.rs"]),
        edge(A, B, Some("identity:key")),
        restore(R, A, B),
        edge(E, B, Some("caller:flag")),
    ]);
    let p = doctor_precision(&l).expect("label overturned, still counted");
    assert!(
        p.detail.contains("identity:key 1/1 correct"),
        "{}",
        p.detail
    );
    assert!(p.detail.contains("re-confirmed"), "{}", p.detail);
}

#[test]
fn auto_readd_does_not_overturn() {
    // the healer re-hides B alone — one more prediction, the label stands
    let l = log(vec![
        row(B, "note", &["src/a.rs"]),
        edge(A, B, Some("identity:key")),
        restore(R, A, B),
        edge(E, B, Some("identity:key")),
    ]);
    let p = doctor_precision(&l).expect("label stands");
    assert!(
        p.detail
            .contains("identity:key 0/1 correct, 1 restored of 2 edges"),
        "{}",
        p.detail
    );
    assert!(p.detail.contains("still open"), "{}", p.detail);
}

#[test]
fn cross_key_groups_by_rule() {
    let l = log(vec![
        row(B, "note", &["src/a.rs"]),
        edge(A, B, Some("identity:key:cross-key")),
        restore(R, A, B),
    ]);
    let p = doctor_precision(&l).expect("cross-key is exposure, not a rule");
    assert!(
        p.detail.contains("identity:key 0/1 correct"),
        "{}",
        p.detail
    );
}
