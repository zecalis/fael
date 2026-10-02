//! `--revisit` (PLAN-fael-row-hygiene chunk 5): date vs text, a due date
//! pins its row first at kickoff even from outside the file filter,
//! `find --revisit` lists, free text only counts.

use super::{ids, row};
use fael_core::*;

// Far past / far future, so `today()` (the real clock) stays deterministic.
const PAST: &str = "2000-01";
const FUTURE: &str = "2999-01";

fn dated(id: &str, kind: &str, files: &[&str], revisit: &str) -> Row {
    let mut r = row(id, kind, files, None);
    r.revisit = Some(revisit.into());
    r
}

fn root() -> std::path::PathBuf {
    let r = std::env::temp_dir().join(format!("fael-revisit-{}", ulid()));
    std::fs::create_dir_all(r.join("src")).unwrap();
    for f in ["src/a.rs", "src/b.rs", "src/c.rs"] {
        std::fs::write(r.join(f), "x").unwrap();
    }
    r
}

fn log() -> Log {
    Log {
        rows: vec![
            dated("D0000000000000000000000010", "note", &["src/a.rs"], PAST),
            row("D0000000000000000000000011", "issue", &["src/b.rs"], None),
            dated("D0000000000000000000000012", "note", &["src/a.rs"], FUTURE),
            dated(
                "D0000000000000000000000013",
                "note",
                &["src/a.rs"],
                "mdl lands",
            ),
        ],
        closes: vec![],
        warnings: vec![],
    }
}

#[test]
fn is_date_takes_padded_yyyy_mm_dd_only() {
    for d in ["2026-09", "2026-09-27", "2000-01-01"] {
        assert!(is_date(d), "{d}");
    }
    for d in [
        "mdl lands",
        "2026-9",
        "2026-09-2",
        "2026-13",
        "2026-00",
        "2026-09-32",
        "2026-09-00",
        "2026/09",
        "",
        "2026-09-27T10:00",
    ] {
        assert!(!is_date(d), "{d}");
    }
}

#[test]
fn due_is_a_date_not_later_than_today() {
    assert!(due("2026-09", "2026-09-27"));
    assert!(due("2026-09-27", "2026-09-27"));
    assert!(!due("2026-09-28", "2026-09-27"));
    assert!(!due("2026-10", "2026-09-27"));
    assert!(!due("mdl lands", "2026-09-27"));
    // a hand-written revisit in `extra` reads like the field
    let mut r = row("D0000000000000000000000014", "note", &["src/a.rs"], None);
    r.extra.insert("revisit".into(), PAST.into());
    assert!(row_due(&r, "2026-09-27"));
}

#[test]
fn kickoff_pins_due_first_and_wakes_outside_the_filter() {
    let r = root();
    let l = log();
    let scoped = Filter {
        files: vec!["src/b.rs".into()],
        ..Filter::default()
    };
    // 10 is due under another path, so it wakes up first; the future date
    // and the free text stay out of a scoped kickoff
    assert_eq!(
        ids(&kickoff(
            &l,
            &scoped,
            &r,
            &Aliases::default(),
            &["PLAN-".into()]
        )),
        ["10", "11"]
    );
    // unscoped: due first, then the rest ranked (issue 11, then notes)
    assert_eq!(
        ids(&kickoff(
            &l,
            &Filter::default(),
            &r,
            &Aliases::default(),
            &["PLAN-".into()]
        )),
        ["10", "11", "13", "12"]
    );
}

#[test]
fn kickoff_hides_closed_due_rows() {
    let r = root();
    let mut l = log();
    l.rows.push(dated(
        "D0000000000000000000000014",
        "note",
        &["src/c.rs"],
        PAST,
    ));
    l.closes
        .push(Row::close("t-0000", "D0000000000000000000000014", "done"));
    assert!(
        !kickoff(
            &l,
            &Filter::default(),
            &r,
            &Aliases::default(),
            &["PLAN-".into()]
        )
        .iter()
        .any(|row| row.id.ends_with("14"))
    );
}

#[test]
fn find_revisit_filters_any_or_substring() {
    let l = log();
    let f = |revisit: &str| {
        ids(&find(
            &l,
            &Filter {
                revisit: Some(revisit.into()),
                ..Filter::default()
            },
        ))
    };
    assert_eq!(f(""), ["13", "12", "10"]);
    assert_eq!(f("2000"), ["10"]);
    assert_eq!(f("MDL"), ["13"]); // case-insensitive
    assert!(f("2998").is_empty());
    // a revisit filter is a real filter (query runs find, not the brief)
    let f = Filter {
        revisit: Some(String::new()),
        ..Filter::default()
    };
    assert!(!f.is_empty());
    let (rows, _, _) = query(&l, &f, &Config::default());
    assert_eq!(ids(&rows), ["13", "12", "10"]);
}

#[test]
fn waiting_counts_free_text_only() {
    let mut l = log();
    // free text on gone files still counts: the count line points at
    // `find --revisit`, which lists it too
    l.rows.push(dated(
        "D0000000000000000000000014",
        "note",
        &["src/gone.rs"],
        "someday",
    ));
    assert_eq!(ids(&waiting(&l)), ["14", "13"]);
    assert!(waiting_line(1).contains("1 row waiting on revisit"));
    assert!(waiting_line(2).contains("2 rows waiting on revisit"));
    assert!(waiting_line(1).contains("fael find --revisit"));
}

#[test]
fn bump_keeps_sets_and_clears_revisit() {
    let dir = std::env::temp_dir().join(format!("fael-bump-revisit-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config::default();
    let st = Stamp {
        by: "tester-0000".into(),
        branch: None,
        sha: None,
    };
    let mut r = Row::new("tester-0000", "note", "sleeper", vec!["src/a.rs".into()]);
    r.revisit = Some(PAST.into());
    let r = add_row(&dir, None, &read(&dir), &cfg, &st, r, None)
        .unwrap()
        .0;
    // absent revisit keeps the old date
    let (b, _, _) = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &r.id,
        BumpOpts {
            held: None,
            to: None,
            urgent: UrgentChange::Keep,
            revisit: None,
        },
    )
    .unwrap();
    assert_eq!(b.revisit.as_deref(), Some(PAST));
    // a value sets it, no supersede round-trip needed
    let (b2, _, _) = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &b.id,
        BumpOpts {
            held: None,
            to: None,
            urgent: UrgentChange::Keep,
            revisit: Some(FUTURE.into()),
        },
    )
    .unwrap();
    assert_eq!(b2.revisit.as_deref(), Some(FUTURE));
    // blank clears it
    let (b3, _, _) = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &b2.id,
        BumpOpts {
            held: None,
            to: None,
            urgent: UrgentChange::Keep,
            revisit: Some("  ".into()),
        },
    )
    .unwrap();
    assert!(b3.revisit.is_none());
}
