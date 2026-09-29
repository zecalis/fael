//! `bump_row` — a new version of an open row: routing/urgency/revisit change,
//! text/files/key never do; the old version hides through `superseded()`.

use fael_core::*;

#[test]
fn close_on_a_bumped_id_points_at_the_newest_version() {
    let dir = std::env::temp_dir().join(format!("fael-close-bumped-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let (cfg, st) = (
        Config::default(),
        Stamp {
            by: "tester-0000".into(),
            branch: None,
            sha: None,
        },
    );
    let mut r = Row::new("tester-0000", "issue", "hot", vec!["src/a.rs".into()]);
    r.urgent = Some(1.0);
    let r = add_row(&dir, None, &read(&dir), &cfg, &st, r, None)
        .unwrap()
        .0;
    let (b, _, _) = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &r.id,
        BumpOpts {
            to: None,
            urgent: UrgentChange::Keep,
            revisit: None,
        },
    )
    .unwrap();
    assert_eq!(b.urgent, Some(1.0));
    // closing the pre-bump id would leave the live version open
    let e = close_row(&dir, None, &read(&dir), &cfg, &st, &r.id, "done").unwrap_err();
    assert!(
        e.contains(&format!("close the newest version {}", b.id)),
        "{e}"
    );
}

#[test]
fn closing_the_newest_version_closes_the_chain_it_supersedes() {
    let dir = std::env::temp_dir().join(format!("fael-close-chain-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config::default();
    let st = Stamp {
        by: "tester-0000".into(),
        branch: None,
        sha: None,
    };
    let bump = |id: &str| {
        bump_row(
            &dir,
            None,
            &read(&dir),
            &cfg,
            &st,
            id,
            BumpOpts {
                to: None,
                urgent: UrgentChange::Keep,
                revisit: None,
            },
        )
        .unwrap()
        .0
    };
    let a = add_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        Row::new("tester-0000", "issue", "hot", vec!["src/a.rs".into()]),
        None,
    )
    .unwrap()
    .0;
    let b = bump(&a.id);
    let c = bump(&b.id);
    // closing C closes A and B too — each gets its own close row, oldest first
    let (row, _, _) = close_row(&dir, None, &read(&dir), &cfg, &st, &c.id, "done").unwrap();
    assert_eq!(row.reference.as_deref(), Some(c.id.as_str()));
    let l = read(&dir);
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
}

#[test]
fn closing_a_superseded_row_past_its_closed_head_repairs_a_stuck_chain() {
    let dir = std::env::temp_dir().join(format!("fael-close-stuck-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config::default();
    let st = Stamp {
        by: "tester-0000".into(),
        branch: None,
        sha: None,
    };
    // the trap as it existed before the chain-close: A bumped to B, B closed
    // alone (an old binary appended just that one close row), so A sits hidden
    // with no close row and no command reached it
    let a = add_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        Row::new("tester-0000", "issue", "hot", vec!["src/a.rs".into()]),
        None,
    )
    .unwrap()
    .0;
    let b = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &a.id,
        BumpOpts {
            to: None,
            urgent: UrgentChange::Keep,
            revisit: None,
        },
    )
    .unwrap()
    .0;
    let old_close = Row::close(&st.by, &b.id, "done");
    fael_core::close(&dir, &old_close, &cfg).unwrap();
    // the head is closed, so the old "close the newest version" reject no
    // longer holds: closing A directly now repairs the stuck chain
    close_row(&dir, None, &read(&dir), &cfg, &st, &a.id, "done").unwrap();
    let l = read(&dir);
    let closed = closed(&l);
    assert!(closed.contains(a.id.as_str()), "{closed:?}");
    assert!(closed.contains(b.id.as_str()), "{closed:?}");
}

#[test]
fn bump_rewrites_only_the_moved_row() {
    let dir = std::env::temp_dir().join(format!("fael-bump-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config::default();
    let st = Stamp {
        by: "tester-0000".into(),
        branch: None,
        sha: None,
    };
    let file = |text: &str, opt: &Urgent| {
        let log = read(&dir);
        let mut r = Row::new("tester-0000", "issue", text, vec!["src/a.rs".into()]);
        r.key = Some("auth:session".into());
        r.urgent = resolve_urgent(&log, opt).unwrap();
        add_row(&dir, None, &log, &cfg, &st, r, None).unwrap().0
    };
    let a = file("A keeps", &Urgent::End); // 1.0
    let b = file("B moves", &Urgent::End); // 2.0
    // done criteria: bump B --urgent-before A with A=1,B=2 → new B=0.5
    let (b2, _, _) = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &b.id,
        BumpOpts {
            to: Some("Ploy".into()),
            urgent: UrgentChange::Before(a.id.clone()),
            revisit: None,
        },
    )
    .unwrap();
    assert_eq!(b2.urgent, Some(0.5));
    assert_eq!(b2.to.as_deref(), Some("ploy")); // lowercased on write
    assert_eq!(b2.text, "B moves");
    assert_eq!(b2.files, ["src/a.rs"]);
    assert_eq!(b2.key.as_deref(), Some("auth:session"));
    assert_eq!(b2.supersedes.as_deref(), Some(b.id.as_str()));
    // the old B left every list; A kept its id and number
    let l = read(&dir);
    // full ids: the 2-char tails from ids() collide by chance (~1 in 500 runs)
    let got: Vec<&str> = find(&l, &Filter::default())
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert!(!got.contains(&b.id.as_str()), "{got:?}");
    let a_still = find(&l, &Filter::default())
        .into_iter()
        .find(|r| r.id == a.id)
        .unwrap();
    assert_eq!(a_still.urgent_value(), Some(1.0));
    // leaving the queue keeps the routing
    let (b3, _, _) = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &b2.id,
        BumpOpts {
            to: None,
            urgent: UrgentChange::Remove,
            revisit: None,
        },
    )
    .unwrap();
    assert!(b3.urgent.is_none());
    assert_eq!(b3.to.as_deref(), Some("ploy"));
}

#[test]
fn bump_rejects_hidden_rows_and_non_issue_urgent() {
    let dir = std::env::temp_dir().join(format!("fael-bump-no-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config::default();
    let st = Stamp {
        by: "tester-0000".into(),
        branch: None,
        sha: None,
    };
    let log = read(&dir);
    let mut d = Row::new("tester-0000", "decision", "locked", vec!["src/a.rs".into()]);
    d.urgent = None;
    let d = add_row(&dir, None, &log, &cfg, &st, d, None).unwrap().0;
    let e = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &d.id,
        BumpOpts {
            to: None,
            urgent: UrgentChange::End,
            revisit: None,
        },
    )
    .unwrap_err();
    assert!(e.contains("urgent is for issues"), "{e}");
    let (gone, _, _) = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &d.id,
        BumpOpts {
            to: Some("ploy".into()),
            urgent: UrgentChange::Keep,
            revisit: None,
        },
    )
    .unwrap();
    assert_eq!(gone.to.as_deref(), Some("ploy"));
    // the old version is superseded — bump the newer one instead
    let e = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &d.id,
        BumpOpts {
            to: None,
            urgent: UrgentChange::Keep,
            revisit: None,
        },
    )
    .unwrap_err();
    assert!(e.contains("already superseded"), "{e}");
    close_row(&dir, None, &read(&dir), &cfg, &st, &gone.id, "done").unwrap();
    let e = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &gone.id,
        BumpOpts {
            to: None,
            urgent: UrgentChange::Keep,
            revisit: None,
        },
    )
    .unwrap_err();
    assert!(e.contains("already closed"), "{e}");
}
