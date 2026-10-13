//! Journal-only writes (PLAN-fael-durable-log chunk 1, tracked store dropped):
//! a row lands in the journal and never in the tree, which only takes rows
//! when there is no journal (no git).

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
fn store_tracked_is_rejected_toward_import_and_local_still_parses() {
    assert!(Config::from_toml("").is_ok());
    assert!(Config::from_toml("store = \"local\"").is_ok());
    let e = Config::from_toml("store = \"tracked\"").unwrap_err();
    assert!(
        e.contains("is gone") && e.contains("fael import <path>") && e.contains("fael-sync"),
        "{e}"
    );
    let e = Config::from_toml("store = \"cloud\"").unwrap_err();
    assert!(e.contains("local"), "{e}");
}

#[test]
fn a_retired_plan_dirs_field_still_parses() {
    // PLAN-fael-plan-focus dropped the active plan line; configs written for
    // it keep loading instead of failing every command
    assert!(Config::from_toml("plan_dirs = [\"docs/plans\"]\nstore = \"local\"").is_ok());
    // decision 01M4SQ9B cut `fael sync`; its `[sync] auto` switch is ignored
    assert!(Config::from_toml("[sync]\nauto = false\n").is_ok());
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
    add_row(
        &fael,
        Some(&journal),
        &read(&fael),
        &cfg,
        &stamp(),
        row(),
        None,
    )
    .unwrap_err();
    // nothing landed anywhere — no tree-only success after a journal failure
    assert!(tree_lines(&d).is_empty());
    assert!(journal_lines(&d).is_empty());
}

#[test]
fn a_journal_write_skips_the_tree() {
    let d = tmp("local");
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
    assert_eq!(journal_lines(&d), vec![r.id.clone()]);
    assert!(
        !fael.join("log").exists(),
        "a journal write must not touch the tree log"
    );
}

#[test]
fn no_journal_falls_back_to_the_tree() {
    let d = tmp("nogit");
    let fael = d.join(".fael");
    let cfg = Config::default();
    let (r, _, _) = add_row(&fael, None, &read(&fael), &cfg, &stamp(), row(), None).unwrap();
    assert_eq!(tree_lines(&d), vec![r.id.clone()]);
}
