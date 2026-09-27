//! `store` + journal-first writes (PLAN-fael-durable-log chunk 1): the journal
//! is the commit point, the tree follows per `store`, and both carry the same
//! line bytes.

use fael_core::*;

fn stamp() -> Stamp {
    Stamp {
        by: "tester-0000".into(),
        branch: Some("feat/x".into()),
        sha: None,
    }
}

fn row() -> Row {
    Row::new("tester-0000", "note", "journal me", vec!["doc:seed".into()])
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("fael-store-{name}-{}", ulid()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn tree_lines(dir: &std::path::Path) -> Vec<String> {
    read(&dir.join(".fael"))
        .rows
        .iter()
        .map(|r| r.id.clone())
        .collect()
}

fn journal_lines(dir: &std::path::Path) -> Vec<String> {
    read(&dir.join("journal"))
        .rows
        .iter()
        .map(|r| r.id.clone())
        .collect()
}

#[test]
fn store_parses_tracked_default_local_and_rejects_the_rest() {
    assert!(matches!(Config::default().store, Store::Tracked));
    assert!(matches!(
        Config::from_toml("").unwrap().store,
        Store::Tracked
    ));
    assert!(matches!(
        Config::from_toml("store = \"local\"").unwrap().store,
        Store::Local
    ));
    assert!(matches!(
        Config::from_toml("store = \"tracked\"").unwrap().store,
        Store::Tracked
    ));
    let e = Config::from_toml("store = \"cloud\"").unwrap_err();
    assert!(e.contains("tracked") && e.contains("local"), "{e}");
}

#[test]
fn tracked_writes_journal_and_tree_with_the_same_bytes() {
    let d = tmp("both");
    let fael = d.join(".fael");
    let journal = d.join("journal");
    let cfg = Config::default();
    let (r, _, warns) = add_row(
        &fael,
        Some(&journal),
        &read(&fael),
        &cfg,
        &stamp(),
        row(),
        None,
    )
    .unwrap();
    assert!(warns.is_empty(), "{warns:?}");
    assert_eq!(tree_lines(&d), vec![r.id.clone()]);
    assert_eq!(journal_lines(&d), vec![r.id.clone()]);
    // same line bytes in both places: one file each, identical content
    let tree_file = std::fs::read_dir(fael.join("log").join("tester-0000"))
        .unwrap()
        .map(|e| std::fs::read(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    let journal_file = std::fs::read_dir(journal.join("log").join("tester-0000"))
        .unwrap()
        .map(|e| std::fs::read(e.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(tree_file.len(), 1);
    assert_eq!(tree_file, journal_file);
}

#[test]
fn tracked_tree_failure_stays_success_with_a_warning() {
    let d = tmp("tree-fail");
    let fael = d.join(".fael");
    let journal = d.join("journal");
    // block the tree month file: a directory where the `.jsonl` must go
    let month = current_month();
    let blocked = fael.join("log").join("tester-0000");
    std::fs::create_dir_all(blocked.join(format!("{month}.jsonl"))).unwrap();
    let cfg = Config::default();
    let (r, _, warns) = add_row(
        &fael,
        Some(&journal),
        &read(&fael),
        &cfg,
        &stamp(),
        row(),
        None,
    )
    .unwrap();
    assert_eq!(warns.len(), 1, "{warns:?}");
    assert!(warns[0].contains("do not retry"), "{warns:?}");
    // journal-only row: durable and listed, tree holds nothing
    assert_eq!(journal_lines(&d), vec![r.id.clone()]);
    assert!(tree_lines(&d).is_empty());
}

#[test]
fn journal_failure_fails_the_whole_write() {
    let d = tmp("journal-fail");
    let fael = d.join(".fael");
    let journal = d.join("journal");
    // block the journal writer dir with a file, so no month file can be made
    let blocked = journal.join("log");
    std::fs::create_dir_all(&blocked).unwrap();
    std::fs::write(blocked.join("tester-0000"), "not a dir").unwrap();
    let cfg = Config::default();
    let e = add_row(
        &fael,
        Some(&journal),
        &read(&fael),
        &cfg,
        &stamp(),
        row(),
        None,
    )
    .unwrap_err();
    assert!(!e.contains("do not retry"), "{e}");
    // nothing landed anywhere — no tree-only success after a journal failure
    assert!(tree_lines(&d).is_empty());
    assert!(journal_lines(&d).is_empty());
}

#[test]
fn local_skips_the_tree() {
    let d = tmp("local");
    let fael = d.join(".fael");
    let journal = d.join("journal");
    let cfg = Config::from_toml("store = \"local\"").unwrap();
    let (r, _, warns) = add_row(
        &fael,
        Some(&journal),
        &read(&fael),
        &cfg,
        &stamp(),
        row(),
        None,
    )
    .unwrap();
    assert!(warns.is_empty(), "{warns:?}");
    assert_eq!(journal_lines(&d), vec![r.id.clone()]);
    assert!(
        !fael.join("log").exists(),
        "local mode must not touch the tree log"
    );
}

#[test]
fn local_without_a_journal_falls_back_to_the_tree() {
    let d = tmp("local-nogit");
    let fael = d.join(".fael");
    let cfg = Config::from_toml("store = \"local\"").unwrap();
    let (r, _, _) = add_row(&fael, None, &read(&fael), &cfg, &stamp(), row(), None).unwrap();
    assert_eq!(tree_lines(&d), vec![r.id.clone()]);
}
