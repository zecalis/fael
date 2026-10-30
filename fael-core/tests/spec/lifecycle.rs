//! c1 lifecycle idempotency (plan fael-secretary): duplicate/retry returns the
//! canonical result with zero new rows; a stale close names the survivor; a
//! second successor never files. Runs against the real log files.

use fael_core::*;
use std::path::{Path, PathBuf};

fn setup(tag: &str) -> (PathBuf, Config, Stamp) {
    let dir = std::env::temp_dir().join(format!("fael-life-{tag}-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let st = Stamp {
        by: "tester-0000".into(),
        branch: None,
        sha: None,
        session: None,
        shown: None,
    };
    (dir, Config::default(), st)
}

fn view(dir: &Path) -> Log {
    fold_bumps(read(dir))
}

fn issue(dir: &Path, cfg: &Config, st: &Stamp, text: &str, prev: Option<&str>) -> Row {
    let r = Row::new("tester-0000", "issue", text, vec!["src/a.rs".into()]);
    add_row(dir, None, &view(dir), cfg, st, r, prev).unwrap().0
}

fn count(dir: &Path) -> (usize, usize) {
    let l = view(dir);
    (l.rows.len(), l.closes.len())
}

#[test]
fn write_replay_close_retry_same_id_zero_rows() {
    let (dir, cfg, st) = setup("close-retry");
    let a = issue(&dir, &cfg, &st, "hot", None);
    let (c1, _, _) = close_row(
        &dir,
        None,
        &view(&dir),
        &cfg,
        &st,
        &a.id,
        "broke → fixed; guard `src/a_test.rs`",
    )
    .unwrap();
    let before = count(&dir);
    // retry with a different reason still names the same close, writes nothing
    let (c2, _, warns) =
        close_row(&dir, None, &view(&dir), &cfg, &st, &a.id, "other words").unwrap();
    assert_eq!(c1.id, c2.id, "T2 retry returns the same close id");
    assert!(
        warns.iter().any(|w| w.contains("not filed again")),
        "{warns:?}"
    );
    assert_eq!(count(&dir), before, "T2 writes zero rows");
}

#[test]
fn write_replay_supersede_retry_single_successor() {
    let (dir, cfg, st) = setup("sup-retry");
    let a = issue(&dir, &cfg, &st, "hot", None);
    let b = issue(&dir, &cfg, &st, "hot v2", Some(&a.id));
    let before = count(&dir);
    // retry with identical content names the same survivor
    let r2 = Row::new("tester-0000", "issue", "hot v2", vec!["src/a.rs".into()]);
    let (got, _, warns) = add_row(&dir, None, &view(&dir), &cfg, &st, r2, Some(&a.id)).unwrap();
    assert_eq!(got.id, b.id, "T5 retry returns the survivor");
    assert!(
        warns.iter().any(|w| w.contains("not filed again")),
        "{warns:?}"
    );
    assert_eq!(count(&dir), before, "T5 writes zero rows");
    // different content files a second edge (multi-edge, `restore --edge`
    // disambiguates): an explicit caller's content is never dropped.
    let rival = Row::new(
        "tester-0000",
        "issue",
        "hot v3 rival",
        vec!["src/a.rs".into()],
    );
    let (got, _, _) = add_row(&dir, None, &view(&dir), &cfg, &st, rival, Some(&a.id)).unwrap();
    assert_ne!(got.id, b.id, "different content files");
    assert_eq!(count(&dir).0, before.0 + 1);
}

#[test]
fn write_replay_race_close_vs_supersede_deterministic() {
    // Order 1: close wins, supersede reopens (one successor, A closed).
    let (dir, cfg, st) = setup("race-1");
    let a = issue(&dir, &cfg, &st, "hot", None);
    close_row(
        &dir,
        None,
        &view(&dir),
        &cfg,
        &st,
        &a.id,
        "broke → fixed; guard `src/a_test.rs`",
    )
    .unwrap();
    let b = issue(&dir, &cfg, &st, "hot v2", Some(&a.id));
    let l = view(&dir);
    assert!(closed(&l).contains(a.id.as_str()));
    assert_eq!(successors(&l).get(a.id.as_str()), Some(&b.id.as_str()));
    // Order 2: supersede wins, stale close names the survivor, then the head closes.
    let (dir, cfg, st) = setup("race-2");
    let a = issue(&dir, &cfg, &st, "hot", None);
    let b = issue(&dir, &cfg, &st, "hot v2", Some(&a.id));
    let before = count(&dir);
    let (got, _, warns) = close_row(
        &dir,
        None,
        &view(&dir),
        &cfg,
        &st,
        &a.id,
        "broke → fixed; guard `src/a_test.rs`",
    )
    .unwrap();
    assert_eq!(got.id, b.id, "T3 stale close names the survivor");
    assert!(
        warns.iter().any(|w| w.contains("not filed again")),
        "{warns:?}"
    );
    assert_eq!(count(&dir), before, "T3 writes zero rows");
    close_row(
        &dir,
        None,
        &view(&dir),
        &cfg,
        &st,
        &b.id,
        "broke → fixed; guard `src/a_test.rs`",
    )
    .unwrap();
    let l = view(&dir);
    assert!(closed(&l).contains(a.id.as_str()) && closed(&l).contains(b.id.as_str()));
    assert_eq!(
        successors(&l).get(a.id.as_str()),
        Some(&b.id.as_str()),
        "single successor"
    );
}

#[test]
fn write_replay_add_close_gates_before_write() {
    // Both gates run before any write: a bad close reason saves neither row.
    let (dir, cfg, st) = setup("gates");
    let add = Row::new("tester-0000", "issue", "hot", vec!["src/a.rs".into()]);
    let target = issue(&dir, &cfg, &st, "target", None);
    let before = count(&dir);
    let bad = add_close(
        &dir,
        &view(&dir),
        &cfg,
        &st,
        add,
        None,
        (&target.id, "no arrow no guard"),
    );
    assert!(bad.is_err(), "close gate rejects first");
    assert_eq!(count(&dir), before, "gates before write: zero rows");
    // a good pair lands atomically under one lock
    let add = Row::new("tester-0000", "issue", "hot2", vec!["src/a.rs".into()]);
    let ((ar, _), (cr, _), _) = add_close(
        &dir,
        &view(&dir),
        &cfg,
        &st,
        add,
        None,
        (&target.id, "broke → fixed; guard `src/a_test.rs`"),
    )
    .unwrap();
    assert!(ar.text.contains("hot2"));
    assert_eq!(cr.reference.as_deref(), Some(target.id.as_str()));
    let (rows, closes) = count(&dir);
    assert_eq!((rows, closes), (before.0 + 1, before.1 + 1));
}
