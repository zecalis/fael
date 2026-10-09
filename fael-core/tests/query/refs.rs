//! Id-reference contract (PLAN-fael-id-refs chunk-0): `looks_like_id` shape,
//! union-scope existence (`ref_state`), prose tokenising (`id_tokens`) —
//! against an in-memory `Log`. No behavior change: nothing here touches
//! `resolve`, text search or the write path.

use fael_core::*;

fn row(id: &str) -> Row {
    Row {
        id: id.into(),
        ts: "2026-09-20T00:00:00Z".into(),
        kind: "note".into(),
        text: format!("text of {id}"),
        files: vec!["src/a.rs".into()],
        ..Row::default()
    }
}

fn log() -> Log {
    Log {
        rows: vec![
            row("01AAAA00000000000000000001"),
            row("01AAAA00000000000000000002"),
            row("01BBBB11111111111111111111"),
        ],
        closes: vec![Row {
            id: "01CCCC22222222222222222222".into(),
            reference: Some("01BBBB11111111111111111111".into()),
            text: "fixed".into(),
            ..Row::default()
        }],
        warnings: vec![],
    }
}

#[test]
fn looks_like_id_accepts_full_ulid_and_prefix() {
    assert!(looks_like_id("01M3M8Y8000000000000000000"));
    assert!(looks_like_id("01M3M8Y8"));
    assert!(looks_like_id("01m3m8y8")); // case-insensitive
    // the second char encodes ms: a 2039-era id starts `02`, still a ULID
    assert!(looks_like_id("02M3M8Y800"));
}

#[test]
fn looks_like_id_rejects_non_shapes() {
    assert!(!looks_like_id("01M3M8Y")); // 7 chars — never a printed prefix
    assert!(!looks_like_id("0123")); // prose number, not something fael printed
    assert!(!looks_like_id("0107544000108")); // all digits: a tax id, not a ULID
    assert!(!looks_like_id("0e0fd51a")); // lowercase hex: a git short sha
    assert!(looks_like_id("0E0FD51A")); // uppercase: what fael prints
    assert!(!looks_like_id("12M3M8Y800")); // first char is 0 until ~year 3084
    assert!(!looks_like_id("M3M8Y80000")); // not a leading 0
    assert!(!looks_like_id("01M3M8Y8IXXXXXXXXXXXXXXXXX")); // I excluded
    assert!(!looks_like_id("01M3M8Y8LXXXXXXXXXXXXXXXX")); // L excluded
    assert!(!looks_like_id("01M3M8Y8OXXXXXXXXXXXXXXXX")); // O excluded
    assert!(!looks_like_id("01M3M8Y8UXXXXXXXXXXXXXXXX")); // U excluded
    assert!(!looks_like_id("01M3M8Y800000000000000000000")); // 27 chars
}

#[test]
fn ref_state_exact_and_unique_prefix_are_one() {
    let log = log();
    assert!(matches!(
        ref_state(&log, "01BBBB11111111111111111111"),
        Ref::One(r) if r.id == "01BBBB11111111111111111111"
    ));
    assert!(matches!(
        ref_state(&log, "01BBBB1111"),
        Ref::One(r) if r.id == "01BBBB11111111111111111111"
    ));
}

#[test]
fn ref_state_shared_prefix_is_many_never_missing() {
    let log = log();
    match ref_state(&log, "01AAAA0000") {
        Ref::Many(rows) => assert_eq!(rows.len(), 2),
        _ => panic!("shared prefix must be Many"),
    }
}

#[test]
fn ref_state_absent_is_missing() {
    let log = log();
    assert!(matches!(ref_state(&log, "01ZZZZ9999"), Ref::Missing));
    assert!(matches!(ref_state(&log, ""), Ref::Missing));
}

#[test]
fn ref_state_close_row_id_is_not_missing() {
    let log = log();
    assert!(matches!(
        ref_state(&log, "01CCCC22222222222222222222"),
        Ref::One(_)
    ));
    assert!(matches!(ref_state(&log, "01CCCC2222"), Ref::One(_)));
}

/// A close row's own id shares the prefix of an open row's id: the row wins —
/// `abbrev()` prints only the rows, so the printed prefix must stay `One`, not
/// turn `Many` because a close row echoes in the wider existence scope.
#[test]
fn ref_state_row_wins_over_a_close_row_collision() {
    let log = Log {
        rows: vec![row("01DDDD00000000000000000001")],
        closes: vec![Row {
            id: "01DDDD00000000000000000002".into(),
            reference: Some("01DDDD00000000000000000001".into()),
            text: "fixed".into(),
            ..Row::default()
        }],
        warnings: vec![],
    };
    assert!(matches!(
        ref_state(&log, "01DDDD00"),
        Ref::One(r) if r.id == "01DDDD00000000000000000001"
    ));
    // the close-only id still exists on its own
    assert!(matches!(
        ref_state(&log, "01DDDD00000000000000000002"),
        Ref::One(_)
    ));
}

#[test]
fn phantom_refs_reports_only_tokens_with_no_row() {
    let log = log();
    assert_eq!(
        phantom_refs(&log, "see 01ZZZZ9999 here"),
        vec!["01ZZZZ9999"]
    );
    // real id, unique prefix, ambiguous prefix, close-row id: all resolve
    assert!(phantom_refs(&log, "follows 01BBBB11111111111111111111").is_empty());
    assert!(phantom_refs(&log, "follows 01BBBB1111").is_empty());
    assert!(phantom_refs(&log, "cites 01AAAA0000 here").is_empty());
    assert!(phantom_refs(&log, "fixed per 01CCCC22222222222222222222").is_empty());
    // punctuation trims, plain words and short numbers never count
    assert_eq!(
        phantom_refs(&log, "(01ZZZZ9999), and 0123 words"),
        vec!["01ZZZZ9999"]
    );
    assert!(phantom_refs(&log, "plain words 0123 supersedes").is_empty());
}

/// Legacy non-ULID ids (fapony-era `mug…`) are outside `looks_like_id`, so
/// prose scanning ignores them, but `ref_state` still resolves one that
/// exists — the documented exemption (PLAN-fael-id-refs §2).
#[test]
fn legacy_non_ulid_ids_resolve_but_are_not_scanned() {
    let mut log = log();
    log.rows.push(row("mugcitsh"));
    assert!(matches!(ref_state(&log, "mugcitsh"), Ref::One(_)));
    assert!(!looks_like_id("mugcitsh"));
    // a fake legacy id is invisible to the scanner, by design
    assert!(phantom_refs(&log, "see mugzzzzz here").is_empty());
}

#[test]
fn id_tokens_trims_punctuation_ignores_words_collapses_dupes() {
    assert_eq!(
        id_tokens("see (01ABCDEFGH), and 01ABCDEFGH again"),
        vec!["01ABCDEFGH"]
    );
    assert_eq!(id_tokens("plain words 0123 supersedes"), Vec::<&str>::new());
    assert_eq!(
        id_tokens("01AAAA0000 01BBBB1111"),
        vec!["01AAAA0000", "01BBBB1111"]
    );
}
