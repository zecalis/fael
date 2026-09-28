//! Push buckets + row cap (PLAN-fael-push-focus chunk 1): `bucket` puts each
//! gathered row in Now | File | Background, `select` cuts to `max_rows`. Plan
//! resolution lives in `plan.rs`.

use super::{ids, log, row};
use fael_core::*;
use std::collections::HashSet;

fn policy(max_rows: usize) -> PushPolicy {
    PushPolicy {
        max_rows,
        budget: 800,
        background: PUSH_BACKGROUND,
    }
}

fn query<'a>(l: &'a Log, f: &str, read: bool) -> Vec<(&'a Row, usize)> {
    push_tiered(l, &[f.to_string()], &Aliases::default(), read)
}

#[test]
fn bucket_now_file_background() {
    let f = Focus::default();
    let issue = row("A0000000000000000000000020", "issue", &["src/a.rs"], None);
    let dec = row(
        "A0000000000000000000000021",
        "decision",
        &["src/a.rs"],
        None,
    );
    // an open issue is Now whatever its tier; a tier-0 decision or note is
    // File; the same-dir and shared-key tiers never render
    assert_eq!(bucket(&issue, 1, &f), Bucket::Now);
    assert_eq!(bucket(&dec, 0, &f), Bucket::File);
    assert_eq!(bucket(&dec, 1, &f), Bucket::Background);
    assert_eq!(bucket(&dec, 2, &f), Bucket::Background);
    // urgent reads as Now too
    let mut urg = row(
        "A0000000000000000000000022",
        "decision",
        &["src/a.rs"],
        None,
    );
    urg.urgent = Some(1.0);
    assert_eq!(bucket(&urg, 1, &f), Bucket::Now);
}

#[test]
fn bucket_focus_signals_are_now() {
    let dec = row(
        "A0000000000000000000000023",
        "decision",
        &["src/a.rs"],
        Some("auth:session"),
    );
    // no focus, tier 1: Background
    assert_eq!(bucket(&dec, 1, &Focus::default()), Bucket::Background);
    // my open key, the session branch, the active plan chunk: Now
    let mut keys = HashSet::new();
    keys.insert("auth:session".to_string());
    assert_eq!(
        bucket(
            &dec,
            1,
            &Focus {
                keys,
                ..Focus::default()
            }
        ),
        Bucket::Now
    );
    let mut branched = dec.clone();
    branched.extra.insert("branch".to_string(), "feat/x".into());
    assert_eq!(
        bucket(
            &branched,
            1,
            &Focus {
                branch: Some("feat/x".into()),
                ..Focus::default()
            }
        ),
        Bucket::Now
    );
    let planned = row(
        "A0000000000000000000000024",
        "note",
        &["src/a.rs"],
        Some("plan:foo:chunk-3"),
    );
    let active = Focus {
        plan: PlanResolution::Active {
            name: "foo".into(),
            chunk: Some(3),
            source: PlanSource::Branch,
        },
        ..Focus::default()
    };
    assert_eq!(bucket(&planned, 1, &active), Bucket::Now);
    // a plan row is Now only for `Active { chunk: Some(n) }` — ambiguous or
    // chunk-less never pulls it out of Background
    let ambiguous = Focus {
        plan: PlanResolution::Ambiguous {
            candidates: vec![PlanCandidate {
                name: "foo".into(),
                chunk: 3,
            }],
        },
        ..Focus::default()
    };
    assert_eq!(bucket(&planned, 1, &ambiguous), Bucket::Background);
    let no_chunk = Focus {
        plan: PlanResolution::Active {
            name: "foo".into(),
            chunk: None,
            source: PlanSource::Declared,
        },
        ..Focus::default()
    };
    assert_eq!(bucket(&planned, 1, &no_chunk), Bucket::Background);
}

#[test]
fn select_caps_rows_issues_first() {
    let mut l = log();
    // 6 tier-0 decisions on the same file (11 is superseded, 10 closed —
    // neither pushes)
    for (i, d) in ["20", "21", "22", "23", "24", "25"].iter().enumerate() {
        let mut r = row(
            &format!("D00000000000000000000000{d}"),
            "decision",
            &["src/a.rs"],
            None,
        );
        r.ts = format!("2026-09-{d}T00:00:0{i}Z");
        l.rows.push(r);
    }
    let tiered = query(&l, "src/a.rs", false);
    // same rows, same order as push — tiers only add information
    assert_eq!(
        tiered.iter().map(|(r, _)| r.id.clone()).collect::<Vec<_>>(),
        push(&l, &["src/a.rs".to_string()], &Aliases::default(), false)
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()
    );
    let sel = select(tiered, &Focus::default(), &policy(5));
    // 8 match (14 exact + 6 new + 13 same-dir): 5 shown with the issue
    // first — L1 ranked it last — and 3 omitted
    assert_eq!(sel.shown.len(), 5);
    assert_eq!(sel.shown[0].kind, "issue");
    assert_eq!(sel.omitted, 3);
}

#[test]
fn select_zero_means_token_budget_only() {
    let mut l = log();
    // a same-dir decision: Background only while the cap is on
    l.rows.push(row(
        "D0000000000000000000000026",
        "decision",
        &["src/c.rs"],
        None,
    ));
    // no row cap: L1's own order stands, every row renders, nothing hidden
    let sel = select(query(&l, "src/a.rs", false), &Focus::default(), &policy(0));
    assert_eq!(ids(&sel.shown), ["14", "13", "26"]);
    assert_eq!(sel.findable_after(3), 0);
    assert_eq!(sel.background_dirs, 0);
    assert!(sel.background_keys.is_empty());
}

#[test]
fn select_counts_background_by_exact_call() {
    let mut l = log();
    // a same-dir decision (tier 1) and a decision on another dir sharing the
    // exact hit's key (tier 2) — both hidden, each with its own exact call
    l.rows.push(row(
        "D0000000000000000000000026",
        "decision",
        &["src/c.rs"],
        None,
    ));
    l.rows.push(row(
        "D0000000000000000000000027",
        "decision",
        &["lib/z.rs"],
        Some("auth:session"),
    ));
    let sel = select(query(&l, "src/a.rs", false), &Focus::default(), &policy(5));
    // the same-dir issue 13 is still Now and shows; 14 the tier-0 File row
    assert_eq!(ids(&sel.shown), ["13", "14"]);
    assert_eq!(sel.omitted, 0);
    assert_eq!(sel.background_dirs, 1);
    assert_eq!(sel.background_keys, [("auth:session".to_string(), 1)]);
}

#[test]
fn select_never_cuts_now_rows() {
    let mut l = log();
    for d in ["30", "31", "32", "33", "34", "35"] {
        l.rows.push(row(
            &format!("E00000000000000000000000{d}"),
            "issue",
            &["src/a.rs"],
            None,
        ));
    }
    let sel = select(query(&l, "src/a.rs", false), &Focus::default(), &policy(5));
    // 7 open issues (6 new + 13) exceed the cap: every one shows, the cap
    // only limits the File ring, so the tier-0 decision 14 is the one cut
    assert_eq!(sel.shown.len(), 7);
    assert!(sel.shown.iter().all(|r| r.kind == "issue"));
    assert_eq!(sel.omitted, 1);
}
