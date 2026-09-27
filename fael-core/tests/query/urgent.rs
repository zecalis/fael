//! urgent queue + the 6-step rank — fractional ordering and one ordering for
//! every list (bump's new versions live in `bump.rs`).

use super::ids;
use fael_core::*;

fn issue(id: &str, urgent: Option<f64>) -> Row {
    Row {
        id: id.into(),
        ts: "2026-09-20T00:00:00Z".into(),
        kind: "issue".into(),
        text: format!("text of {id}"),
        files: vec!["src/a.rs".into()],
        urgent,
        ..Row::default()
    }
}

fn queue() -> Log {
    Log {
        rows: vec![
            issue("U0000000000000000000000001", Some(2.0)),
            issue("U0000000000000000000000002", Some(1.0)),
        ],
        closes: vec![],
        warnings: vec![],
    }
}

#[test]
fn urgent_end_is_max_plus_one() {
    let empty = Log::default();
    assert_eq!(resolve_urgent(&empty, &Urgent::End).unwrap(), Some(1.0));
    let l = queue();
    assert_eq!(resolve_urgent(&l, &Urgent::End).unwrap(), Some(3.0));
    assert_eq!(resolve_urgent(&l, &Urgent::Unset).unwrap(), None);
}

#[test]
fn urgent_before_is_the_midpoint_above() {
    let l = queue();
    // the top halves (the queue starts at 1, so halving never crosses zero)
    assert_eq!(
        resolve_urgent(&l, &Urgent::Before("U0000000000000000000000002".into())).unwrap(),
        Some(0.5)
    );
    assert_eq!(
        resolve_urgent(&l, &Urgent::Before("U0000000000000000000000001".into())).unwrap(),
        Some(1.5)
    );
}

#[test]
fn urgent_before_a_tie_skips_to_the_next_smaller_number() {
    // X=1, then Z=2 and Y=2 tied (two writers filed --urgent at once)
    let l = Log {
        rows: vec![
            issue("U0000000000000000000000001", Some(1.0)),
            issue("U0000000000000000000000002", Some(2.0)),
            issue("U0000000000000000000000003", Some(2.0)),
        ],
        ..Log::default()
    };
    // before Y (lower id, sorts after Z): midpoint with X, not 2 - 1 = 1
    assert_eq!(
        resolve_urgent(&l, &Urgent::Before("U0000000000000000000000002".into())).unwrap(),
        Some(1.5)
    );
}

#[test]
fn urgent_before_rejects_non_queue_rows() {
    let mut l = queue();
    l.rows.push(issue("U0000000000000000000000003", None));
    let e = resolve_urgent(&l, &Urgent::Before("U0000000000000000000000003".into())).unwrap_err();
    assert!(e.contains("has no number"), "{e}");
    let e = resolve_urgent(&l, &Urgent::Before("U0000000000000000000000009".into())).unwrap_err();
    assert!(e.contains("no row"), "{e}");
    l.closes
        .push(Row::close("t-0000", "U0000000000000000000000002", "done"));
    let e = resolve_urgent(&l, &Urgent::Before("U0000000000000000000000002".into())).unwrap_err();
    assert!(e.contains("closed or superseded"), "{e}");
}

/// A row with every ranking signal set — the 6-step key reads it directly.
fn full(
    id: &str,
    kind: &str,
    ts: &str,
    to: Option<&str>,
    urgent: Option<f64>,
    files: &[&str],
) -> Row {
    Row {
        id: id.into(),
        ts: ts.into(),
        kind: kind.into(),
        text: format!("text of {id}"),
        files: files.iter().map(|s| s.to_string()).collect(),
        to: to.map(String::from),
        urgent,
        ..Row::default()
    }
}

fn open_log(rows: Vec<Row>) -> Log {
    Log {
        rows,
        closes: vec![],
        warnings: vec![],
    }
}

#[test]
fn rank_to_reader_beats_urgent() {
    let l = open_log(vec![
        full(
            "R0000000000000000000000001",
            "issue",
            "2026-09-20T00:00:00Z",
            None,
            Some(1.0),
            &["a.rs"],
        ),
        full(
            "R0000000000000000000000002",
            "issue",
            "2026-09-10T00:00:00Z",
            Some("ploy"),
            None,
            &["a.rs"],
        ),
    ]);
    let got = ranked(l.rows.iter().collect(), Some("ploy-1a2b"), |_| 0, fresh_ts);
    assert_eq!(ids(&got), ["02", "01"]);
}

#[test]
fn rank_urgent_beats_match_tier() {
    let l = open_log(vec![
        full(
            "R0000000000000000000000001",
            "note",
            "2026-09-10T00:00:00Z",
            None,
            Some(1.0),
            &["src/b.rs"],
        ),
        full(
            "R0000000000000000000000002",
            "issue",
            "2026-09-10T00:00:00Z",
            None,
            None,
            &["src/a.rs"],
        ),
    ]);
    // exact-file loses to same-dir once the same-dir row is urgent
    let got = push(&l, &["src/a.rs".to_string()], &Aliases::default(), false);
    assert_eq!(ids(&got), ["01", "02"]);
}

#[test]
fn rank_kind_beats_freshness() {
    let l = open_log(vec![
        full(
            "R0000000000000000000000001",
            "note",
            "2026-09-20T00:00:00Z",
            None,
            None,
            &["a.rs"],
        ),
        full(
            "R0000000000000000000000002",
            "issue",
            "2026-09-10T00:00:00Z",
            None,
            None,
            &["a.rs"],
        ),
    ]);
    assert_eq!(ids(&find(&l, &Filter::default())), ["02", "01"]);
}

#[test]
fn rank_freshness_beats_id() {
    let l = open_log(vec![
        full(
            "B0000000000000000000000001",
            "note",
            "2026-09-10T00:00:00Z",
            None,
            None,
            &["a.rs"],
        ),
        full(
            "A0000000000000000000000002",
            "note",
            "2026-09-20T00:00:00Z",
            None,
            None,
            &["a.rs"],
        ),
    ]);
    assert_eq!(ids(&find(&l, &Filter::default())), ["02", "01"]);
}

#[test]
fn rank_id_desc_breaks_full_ties() {
    let l = open_log(vec![
        full(
            "A0000000000000000000000001",
            "note",
            "2026-09-10T00:00:00Z",
            None,
            None,
            &["a.rs"],
        ),
        full(
            "A0000000000000000000000002",
            "note",
            "2026-09-10T00:00:00Z",
            None,
            None,
            &["a.rs"],
        ),
    ]);
    assert_eq!(ids(&find(&l, &Filter::default())), ["02", "01"]);
}
