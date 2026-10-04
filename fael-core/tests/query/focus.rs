//! Push buckets + row cap (PLAN-fael-push-focus chunk 1): `bucket` puts each
//! gathered row in Now | File | Background, `select` cuts to `max_rows`.

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
    // an open issue on the file is Now, a tier-0 decision or note is File;
    // the same-dir and shared-key tiers never render, issues included
    assert_eq!(bucket(&issue, 0, &f), Bucket::Now);
    assert_eq!(bucket(&issue, 1, &f), Bucket::Background);
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
    // my open key, the session branch: Now
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
    // a plan-keyed row filed on another branch is no signal: fael never infers
    // the session's plan (PLAN-fael-plan-focus) — fapony asks for its keys
    let mut planned = row(
        "A0000000000000000000000024",
        "note",
        &["src/a.rs"],
        Some("plan:foo:chunk-3"),
    );
    planned
        .extra
        .insert("branch".to_string(), "feat/old".into());
    let rows = [&planned];
    assert_eq!(
        bucket(&planned, 1, &Focus::from_rows(Some("feat/x"), &rows)),
        Bucket::Background
    );
}

#[test]
fn focus_keys_stay_on_the_session_branch() {
    // a plan key is an ordinary key: Now only when this branch filed it
    let mut mine = row(
        "A0000000000000000000000031",
        "note",
        &["src/a.rs"],
        Some("plan:foo:chunk-3"),
    );
    mine.extra.insert("branch".into(), "feat/x".into());
    let mut other = row(
        "A0000000000000000000000032",
        "note",
        &["src/a.rs"],
        Some("plan:bar:chunk-9"),
    );
    other.extra.insert("branch".into(), "other".into());
    let rows = [&mine, &other];
    let f = Focus::from_rows(Some("feat/x"), &rows);
    assert!(f.keys.contains("plan:foo:chunk-3"));
    assert!(!f.keys.contains("plan:bar:chunk-9"));
    // no branch at all: no focus
    assert!(Focus::from_rows(None, &rows).keys.is_empty());
}

#[test]
fn focus_file_from_an_older_fael_still_reads() {
    // #60/#61 wrote a `plan` field — dropping it must not reset the Focus
    let old = r#"{"branch":"feat/x","plan":{"name":"foo","chunk":3,"path":null},"keys":["a:b"]}"#;
    let f: Focus = serde_json::from_str(old).expect("old focus file parses");
    assert_eq!(f.branch.as_deref(), Some("feat/x"));
    assert!(f.keys.contains("a:b"));
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
    l.rows.push(row(
        "D0000000000000000000000029",
        "issue",
        &["src/a.rs"],
        None,
    ));
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
    // 9 match (14 exact + 6 new + issue 29 + 13 same-dir): 5 shown with the
    // file's issue first, 3 omitted; the same-dir issue 13 is only counted
    assert_eq!(sel.shown.len(), 5);
    assert_eq!(sel.shown[0].id, "D0000000000000000000000029");
    assert_eq!(sel.omitted, 3);
    assert_eq!(sel.background_dirs, 1);
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
}

#[test]
fn select_counts_background_by_exact_call() {
    let mut l = log();
    // a same-dir decision (tier 1) and a decision on another dir sharing the
    // exact hit's key (tier 2) — the vela case (decision push:shared-key-siblings):
    // with the key outside Focus the sibling is neither pushed nor counted
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
    // 14 the tier-0 File row shows; the same-dir issue 13 and decision 26
    // are counted for the dir call, never pushed
    assert_eq!(ids(&sel.shown), ["14"]);
    assert_eq!(sel.omitted, 0);
    assert_eq!(sel.background_dirs, 2);
    // the same key in Focus: the sibling is this session's work, so it shows
    let sel = select(
        query(&l, "src/a.rs", false),
        &focus_on("auth:session"),
        &policy(5),
    );
    assert!(
        ids(&sel.shown).contains(&"27".to_string()),
        "{:?}",
        ids(&sel.shown)
    );
}

fn focus_on(key: &str) -> Focus {
    Focus {
        keys: [key.to_string()].into(),
        ..Focus::default()
    }
}

#[test]
fn hidden_routes_the_budget_cut_by_tier() {
    let mut l = log();
    // a same-dir (tier 1) issue is counted for the dir call; a shared-key
    // (tier 2) issue on a Focus key is Now and only the token budget can cut
    // it — each names its own find call (the dir / the key), never `--files <f>`
    l.rows.push(row(
        "D0000000000000000000000026",
        "issue",
        &["src/c.rs"],
        None,
    ));
    l.rows.push(row(
        "D0000000000000000000000027",
        "issue",
        &["lib/z.rs"],
        Some("auth:session"),
    ));
    let sel = select(
        query(&l, "src/a.rs", false),
        &focus_on("auth:session"),
        &policy(5),
    );
    // render said the first row only; the rest were cut by the budget
    let h = sel.hidden(1);
    // the Focus key makes tier-0 row 14 Now, so it renders first; both
    // same-dir issues (13, 26) were counted, never shown
    assert_eq!(h.dirs, 2, "the same-dir issues need the dir call: {h:?}");
    assert_eq!(h.keys, [("auth:session".to_string(), 1)]);
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
    // 6 open issues on the file exceed the cap: every one shows, the cap
    // only limits the File ring, so the tier-0 decision 14 is the one cut
    assert_eq!(sel.shown.len(), 6);
    assert!(sel.shown.iter().all(|r| r.kind == "issue"));
    assert_eq!(sel.omitted, 1);
}

#[test]
fn select_hub_file_peeks_past_now_rows() {
    // a plan cited by more rows than PUSH_HUB_ROWS: freshness alone would
    // fill the cap with off-topic rows, yet (0 of 29) told the agent nothing —
    // the issue and the Focus-keyed row render, then PUSH_HUB_PEEK File rows,
    // a parked one (revisit waiting) ahead of the freshest
    let hub = |n: usize| {
        let mut l = Log::default();
        for i in 0..n {
            let mut r = row(
                &format!("D00000000000000000000001{i:02}"),
                "decision",
                &["PLAN.md"],
                None,
            );
            if i == 0 {
                r.revisit = Some("when print pr2 starts".into());
            }
            l.rows.push(r);
        }
        l.rows.push(row(
            "I0000000000000000000000020",
            "issue",
            &["PLAN.md"],
            None,
        ));
        l.rows.push(row(
            "K0000000000000000000000021",
            "decision",
            &["PLAN.md"],
            Some("credit:ledger"),
        ));
        l
    };
    let focus = Focus {
        branch: None,
        keys: HashSet::from(["credit:ledger".to_string()]),
    };
    let l = hub(PUSH_HUB_ROWS + 1);
    let sel = select(query(&l, "PLAN.md", true), &focus, &policy(5));
    assert_eq!(ids(&sel.shown), ["20", "21", "00", "08", "07"]);
    assert_eq!(sel.omitted, PUSH_HUB_ROWS + 1 - PUSH_HUB_PEEK);
    // a wider cap still stops at the peek on a hub
    let sel = select(query(&l, "PLAN.md", true), &focus, &policy(20));
    assert_eq!(sel.shown.len(), 2 + PUSH_HUB_PEEK);
    // at the threshold the file is no hub: the cap fills as before
    let l = hub(PUSH_HUB_ROWS);
    let sel = select(query(&l, "PLAN.md", true), &focus, &policy(5));
    assert_eq!(sel.shown.len(), 5);
    assert_eq!(sel.omitted, PUSH_HUB_ROWS - 3);
}
