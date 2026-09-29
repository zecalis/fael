//! Restore edges (`fael restore`): `superseded()` is every supersede edge
//! minus the reverted ones; a restore row is a carrier, never a result; the
//! raw edge still names its target — what an old reader hides by (over-hide,
//! intended, format.md §Restore).

use super::{ids, row};
use fael_core::*;

const A: &str = "C0000000000000000000000021";
const B: &str = "C0000000000000000000000022";
const C: &str = "C0000000000000000000000023";

/// B hidden by A→B, plus the restore row reverting it.
fn log_restored() -> Log {
    let mut sup = row(A, "note", &["src/a.rs"], Some("auth:session"));
    sup.supersedes = Some(B.into());
    let target = row(B, "note", &["src/a.rs"], Some("auth:session"));
    let mut back = Row::restored("me", A, B);
    back.id = "C0000000000000000000000024".into();
    Log {
        rows: vec![target, sup, back],
        ..Log::default()
    }
}

#[test]
fn superseded_is_edges_minus_reverted() {
    let l = log_restored();
    assert_eq!(reverted(&l), std::collections::HashSet::from([A]));
    assert!(superseded(&l).is_empty(), "B is open again");
    assert_eq!(ids(&find(&l, &Filter::default())), ["22", "21"]);
}

#[test]
fn second_active_edge_still_hides() {
    let l = log_restored();
    let mut l2 = l.clone();
    let mut sup2 = row(C, "note", &["src/a.rs"], Some("auth:session"));
    sup2.supersedes = Some(B.into());
    l2.rows.push(sup2);
    // only A→B reverted: C→B still hides B
    assert_eq!(ids(&find(&l2, &Filter::default())), ["23", "21"]);
    assert!(superseded(&l2).contains(B));
}

#[test]
fn restore_row_is_carrier_never_a_result_no_bump() {
    let r = Row::restored("me", A, B);
    assert_eq!(r.v, Some(1), "no v bump — intended degrade");
    assert!(is_carrier_row(&r));
    let l = log_restored();
    let all = Filter {
        all: true,
        ..Filter::default()
    };
    // even --all never lists it: a carrier is never a result
    assert_eq!(ids(&find(&l, &all)), ["22", "21"]);
}

#[test]
fn raw_edge_still_names_target_for_old_readers() {
    // an old reader subtracts nothing: the edge is still there, so B stays
    // hidden for it (over-hide, intended — format.md §Restore)
    let l = log_restored();
    let edges: Vec<&str> = l
        .rows
        .iter()
        .filter_map(|r| r.supersedes.as_deref())
        .collect();
    assert_eq!(edges, [B]);
}

#[test]
fn restore_text_names_both_ends_in_full_no_phantom() {
    let l = log_restored();
    let back = &l.rows[2];
    assert!(back.text.contains(A) && back.text.contains(B));
    assert!(phantom_refs(&l, &back.text).is_empty());
}

#[test]
fn keyed_carrier_never_pushes() {
    // a kindless/fileless row with a key shares it with an exact hit — tier 2
    // would take it, so the carrier guard must run first (01M3PF10)
    let keyed = Row {
        id: "C0000000000000000000000025".into(),
        text: "restore edge".into(),
        key: Some("auth:session".into()),
        ..Row::default()
    };
    let mut l = log_restored();
    l.rows.push(keyed);
    let got: Vec<String> = push(&l, &["src/a.rs".to_string()], &Aliases::default(), false)
        .iter()
        .map(|r| r.id[24..].to_string())
        .collect();
    assert!(!got.iter().any(|id| id == "25"), "{got:?}");
}
