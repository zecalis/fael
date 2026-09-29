//! Carrier rows (no kind, no files) are never results; legacy rows (kind, no
//! files) stay results and moved rows stay skipped — the carrier rule is pure
//! forward-compat for rows no old reader knows.

use super::{ids, row};
use fael_core::*;

fn carrier(id: &str) -> Row {
    Row {
        id: id.into(),
        text: "restore edge T123".into(),
        ..Row::default()
    }
}

fn log_with_carrier() -> Log {
    let mut moved = Row::moved("me", "src/old.rs", "src/new.rs");
    moved.id = "C0000000000000000000000023".into();
    Log {
        rows: vec![
            row("C0000000000000000000000021", "note", &["src/a.rs"], None),
            row("C0000000000000000000000022", "note", &[], None), // legacy: kind, no files
            moved,
            carrier("C0000000000000000000000024"),
        ],
        ..Log::default()
    }
}

#[test]
fn carrier_is_never_a_result_legacy_stays() {
    let l = log_with_carrier();
    assert_eq!(ids(&find(&l, &Filter::default())), ["22", "21"]);
    let all = Filter {
        all: true,
        ..Filter::default()
    };
    assert_eq!(ids(&find(&l, &all)), ["22", "21"]);
}

/// The carrier rule is a reader contract, not a `find` filter: a keyed
/// carrier must not push either (it shares a key with a real row on the file,
/// which would otherwise land it at tier 2).
#[test]
fn push_skips_a_keyed_carrier() {
    let present = row(
        "C0000000000000000000000021",
        "note",
        &["src/a.rs"],
        Some("k:a"),
    );
    let mut carrier = carrier("C0000000000000000000000024");
    carrier.key = Some("k:a".into());
    let l = Log {
        rows: vec![present, carrier],
        ..Log::default()
    };
    let pushed = push_tiered(&l, &["src/a.rs".to_string()], &Aliases::default(), true);
    let rows: Vec<&Row> = pushed.iter().map(|(r, _)| *r).collect();
    assert_eq!(ids(&rows), ["21"]);
}
