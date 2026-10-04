//! `bump_row` — moves an open row under its own id: one bump event appended,
//! routing/urgency/revisit change, text/files/key never do, and
//! `fold_bumps` lays the newest event onto the row. Supersede chains (an
//! `add --supersedes`, or a bump from before events) close as one.

use fael_core::*;
use std::path::{Path, PathBuf};

fn setup(tag: &str) -> (PathBuf, Config, Stamp) {
    let dir = std::env::temp_dir().join(format!("fael-{tag}-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let st = Stamp {
        by: "tester-0000".into(),
        branch: None,
        sha: None,
    };
    (dir, Config::default(), st)
}

/// The log as queries see it: bump events folded onto their rows.
fn view(dir: &Path) -> Log {
    fold_bumps(read(dir))
}

fn keep() -> BumpOpts {
    BumpOpts {
        held: None,
        to: None,
        urgent: UrgentChange::Keep,
        revisit: None,
        fh: None,
    }
}

fn bump(dir: &Path, cfg: &Config, st: &Stamp, id: &str, o: BumpOpts) -> Result<Row, String> {
    bump_row(dir, None, &view(dir), cfg, st, id, o).map(|r| r.0)
}

/// An issue on `src/a.rs`, superseding `prev` when given.
fn issue(dir: &Path, cfg: &Config, st: &Stamp, text: &str, prev: Option<&str>) -> Row {
    let r = Row::new("tester-0000", "issue", text, vec!["src/a.rs".into()]);
    add_row(dir, None, &view(dir), cfg, st, r, prev).unwrap().0
}

#[test]
fn bump_keeps_the_id_and_folds_on_read() {
    let (dir, cfg, st) = setup("bump-id");
    let r = issue(&dir, &cfg, &st, "hot", None);
    let b = bump(
        &dir,
        &cfg,
        &st,
        &r.id,
        BumpOpts {
            to: Some("ploy".into()),
            ..keep()
        },
    )
    .unwrap();
    assert_eq!(b.id, r.id);
    assert!(b.supersedes.is_none());
    // the raw log holds the row and one carrier naming it; the view folds
    let raw = read(&dir);
    assert_eq!(raw.rows.len(), 2);
    let ev = raw.rows.iter().find(|x| x.bumps.is_some()).unwrap();
    assert_eq!(ev.bumps.as_deref(), Some(r.id.as_str()));
    assert!(ev.kind.is_empty() && ev.files.is_empty());
    let l = view(&dir);
    let listed = find(&l, &Filter::default());
    assert_eq!(listed.len(), 1, "the carrier never lists");
    assert_eq!(listed[0].id, r.id);
    assert_eq!(listed[0].to.as_deref(), Some("ploy"));
    // the id an agent copied before the bump still closes the row
    close_row(&dir, None, &view(&dir), &cfg, &st, &r.id, "done").unwrap();
    assert!(find(&view(&dir), &Filter::default()).is_empty());
}

#[test]
fn closing_the_newest_version_closes_the_chain_it_supersedes() {
    let (dir, cfg, st) = setup("close-chain");
    let a = issue(&dir, &cfg, &st, "hot", None);
    let b = issue(&dir, &cfg, &st, "hot, v2", Some(&a.id));
    let c = issue(&dir, &cfg, &st, "hot, v3", Some(&b.id));
    // closing C closes A and B too — each gets its own close row, oldest first
    let (row, _, _) = close_row(&dir, None, &view(&dir), &cfg, &st, &c.id, "done").unwrap();
    assert_eq!(row.reference.as_deref(), Some(c.id.as_str()));
    let l = view(&dir);
    let closed = closed(&l);
    for id in [&a.id, &b.id, &c.id] {
        assert!(closed.contains(id.as_str()), "{id} not closed: {closed:?}");
    }
    // every version left the default list; `--all` still shows them marked closed
    assert!(find(&l, &Filter::default()).is_empty());
    let all: Vec<&Row> = find(
        &l,
        &Filter {
            all: true,
            ..Filter::default()
        },
    );
    for id in [&a.id, &b.id, &c.id] {
        assert!(all.iter().any(|r| &r.id == id), "{id} gone from --all");
    }
    // closing a version below the open head points at the head instead
    let (dir, cfg, st) = setup("close-below");
    let a = issue(&dir, &cfg, &st, "hot", None);
    let b = issue(&dir, &cfg, &st, "hot, v2", Some(&a.id));
    let e = close_row(&dir, None, &view(&dir), &cfg, &st, &a.id, "done").unwrap_err();
    assert!(
        e.contains(&format!("close the newest version {}", b.id)),
        "{e}"
    );
}

#[test]
fn closing_a_superseded_row_past_its_closed_head_repairs_a_stuck_chain() {
    let (dir, cfg, st) = setup("close-stuck");
    // the trap as it existed before the chain-close: A superseded by B, B
    // closed alone (an old binary appended just that one close row), so A
    // sits hidden with no close row and no command reached it
    let a = issue(&dir, &cfg, &st, "hot", None);
    let b = issue(&dir, &cfg, &st, "hot, v2", Some(&a.id));
    let old_close = Row::close(&st.by, &b.id, "done");
    fael_core::close(&dir, &old_close, &cfg).unwrap();
    // the head is closed, so the old "close the newest version" reject no
    // longer holds: closing A directly now repairs the stuck chain
    close_row(&dir, None, &view(&dir), &cfg, &st, &a.id, "done").unwrap();
    let l = view(&dir);
    let closed = closed(&l);
    assert!(closed.contains(a.id.as_str()), "{closed:?}");
    assert!(closed.contains(b.id.as_str()), "{closed:?}");
}

#[test]
fn bump_rewrites_only_the_moved_row() {
    let (dir, cfg, st) = setup("bump");
    let file = |text: &str, opt: &Urgent| {
        let log = view(&dir);
        let mut r = Row::new("tester-0000", "issue", text, vec!["src/a.rs".into()]);
        r.key = Some("auth:session".into());
        r.urgent = resolve_urgent(&log, opt).unwrap();
        add_row(&dir, None, &log, &cfg, &st, r, None).unwrap().0
    };
    let a = file("A keeps", &Urgent::End); // 1.0
    let b = file("B moves", &Urgent::End); // 2.0
    // done criteria: bump B --urgent-before A with A=1,B=2 → B=0.5
    let b2 = bump(
        &dir,
        &cfg,
        &st,
        &b.id,
        BumpOpts {
            to: Some("Ploy".into()),
            urgent: UrgentChange::Before(a.id.clone()),
            ..keep()
        },
    )
    .unwrap();
    assert_eq!(b2.urgent, Some(0.5));
    assert_eq!(b2.to.as_deref(), Some("ploy")); // lowercased on write
    assert_eq!(b2.text, "B moves");
    assert_eq!(b2.files, ["src/a.rs"]);
    assert_eq!(b2.key.as_deref(), Some("auth:session"));
    assert_eq!(b2.id, b.id);
    // the list holds B once, moved; A kept its number
    let l = view(&dir);
    let listed = find(&l, &Filter::default());
    assert_eq!(listed.len(), 2);
    let pick = |id: &str| listed.iter().find(|r| r.id == id).unwrap();
    assert_eq!(pick(&b.id).urgent_value(), Some(0.5));
    assert_eq!(pick(&a.id).urgent_value(), Some(1.0));
    // leaving the queue keeps the routing — the newest event wins
    let b3 = bump(
        &dir,
        &cfg,
        &st,
        &b.id,
        BumpOpts {
            urgent: UrgentChange::Remove,
            ..keep()
        },
    )
    .unwrap();
    assert!(b3.urgent.is_none());
    assert_eq!(b3.to.as_deref(), Some("ploy"));
    let l = view(&dir);
    let folded = find(&l, &Filter::default())
        .into_iter()
        .find(|r| r.id == b.id)
        .unwrap();
    assert!(folded.urgent.is_none());
    assert_eq!(folded.to.as_deref(), Some("ploy"));
}

#[test]
fn bump_rejects_hidden_rows_and_non_issue_urgent() {
    let (dir, cfg, st) = setup("bump-no");
    let d = Row::new("tester-0000", "decision", "locked", vec!["src/a.rs".into()]);
    let d = add_row(&dir, None, &view(&dir), &cfg, &st, d, None)
        .unwrap()
        .0;
    let urgent = BumpOpts {
        urgent: UrgentChange::End,
        ..keep()
    };
    let e = bump(&dir, &cfg, &st, &d.id, urgent).unwrap_err();
    assert!(e.contains("urgent is for issues"), "{e}");
    // the old version is superseded — bump the newer one instead
    let newer = Row::new(
        "tester-0000",
        "decision",
        "locked v2",
        vec!["src/a.rs".into()],
    );
    let newer = add_row(&dir, None, &view(&dir), &cfg, &st, newer, Some(&d.id))
        .unwrap()
        .0;
    let e = bump(&dir, &cfg, &st, &d.id, keep()).unwrap_err();
    assert!(e.contains("already superseded"), "{e}");
    close_row(&dir, None, &view(&dir), &cfg, &st, &newer.id, "done").unwrap();
    let e = bump(&dir, &cfg, &st, &newer.id, keep()).unwrap_err();
    assert!(e.contains("already closed"), "{e}");
}

#[test]
fn an_event_ahead_of_its_row_still_folds_and_an_orphan_is_ignored() {
    // sync can land the event before the row it names, or the row may never
    // arrive (another writer's ref not fetched yet): fold by id, not by order
    let row = Row::new("tester-0000", "issue", "hot", vec!["src/a.rs".into()]);
    let mut ev = Row::bumped("tester-0000", &row.id);
    ev.to = Some("ploy".into());
    let mut orphan = Row::bumped("tester-0000", "01M0000000000000000000GONE");
    orphan.to = Some("vela".into());
    let log = Log {
        rows: vec![ev, orphan, row.clone()],
        closes: vec![],
        warnings: vec![],
    };
    let l = fold_bumps(log);
    let listed = find(&l, &Filter::default());
    assert_eq!(listed.len(), 1, "carriers never list: {listed:?}");
    assert_eq!(listed[0].id, row.id);
    assert_eq!(listed[0].to.as_deref(), Some("ploy"));
}

#[test]
fn a_bump_on_the_head_of_a_legacy_chain_keeps_its_id_and_closes_the_chain() {
    // a log written before bump events: B is a supersede-bump of A. A new
    // binary bumps B in place, and closing B still closes A with it
    let (dir, cfg, st) = setup("bump-legacy");
    let a = issue(&dir, &cfg, &st, "hot", None);
    let b = issue(&dir, &cfg, &st, "hot", Some(&a.id));
    let up = BumpOpts {
        to: Some("ploy".into()),
        ..keep()
    };
    let moved = bump(&dir, &cfg, &st, &b.id, up).unwrap();
    assert_eq!(moved.id, b.id);
    let l = view(&dir);
    let listed = find(&l, &Filter::default());
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].id, b.id);
    assert_eq!(listed[0].to.as_deref(), Some("ploy"));
    assert_eq!(listed[0].supersedes.as_deref(), Some(a.id.as_str()));
    close_row(&dir, None, &l, &cfg, &st, &b.id, "done").unwrap();
    let l = view(&dir);
    let closed = closed(&l);
    assert!(closed.contains(a.id.as_str()) && closed.contains(b.id.as_str()));
}

#[test]
fn a_bump_always_sorts_after_the_rows_last_event() {
    // two bumps in one ms used to order by the ULID's random half; an event
    // stamped ahead (clock skew) stands in for that tie deterministically
    let (dir, cfg, st) = setup("bump-tie");
    let r = issue(&dir, &cfg, &st, "hot", None);
    let mut ahead = Row::bumped("tester-0000", &r.id);
    ahead.id = ulid_at(now_ms() + 60_000);
    ahead.to = Some("vela".into());
    append(&dir, &ahead, false).unwrap();
    let up = BumpOpts {
        to: Some("ploy".into()),
        ..keep()
    };
    let b = bump(&dir, &cfg, &st, &r.id, up).unwrap();
    assert_eq!(b.to.as_deref(), Some("ploy"));
    let l = view(&dir);
    let listed = find(&l, &Filter::default());
    assert_eq!(
        listed[0].to.as_deref(),
        Some("ploy"),
        "the newest bump wins"
    );
}

#[test]
fn close_and_bump_resolve_past_a_bump_event_sharing_the_prefix() {
    // a bump right after the row (a script that adds then claims) can share
    // the short id an earlier output printed: close and bump look at content
    // rows only, so that id still names the row — and an event id names none
    let (dir, cfg, st) = setup("bump-prefix");
    let r = issue(&dir, &cfg, &st, "hot", None);
    let mut ev = Row::bumped("tester-0000", &r.id);
    let last = if r.id.ends_with('0') { "1" } else { "0" };
    ev.id = format!("{}{last}", &r.id[..25]);
    ev.to = Some("ploy".into());
    append(&dir, &ev, false).unwrap();
    let short = &r.id[..25];
    let l = view(&dir);
    assert!(resolve(&l, short).is_err(), "the raw lookup sees two rows");
    assert_eq!(resolve_row(&l, short).unwrap().id, r.id);
    let e = resolve_row(&l, &ev.id).unwrap_err();
    assert!(e.contains("no row"), "{e}");
    let b = bump(&dir, &cfg, &st, short, keep()).unwrap();
    assert_eq!(b.id, r.id);
    let (c, _, _) = close_row(&dir, None, &view(&dir), &cfg, &st, short, "done").unwrap();
    assert_eq!(c.reference.as_deref(), Some(r.id.as_str()));
}
