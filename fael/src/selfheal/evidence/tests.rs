use super::*;

fn row(id: &str, by: &str, kind: &str, text: &str, files: &[&str], key: Option<&str>) -> core::Row {
    let mut r = core::Row::new(
        by,
        kind,
        text,
        files.iter().map(|s| s.to_string()).collect(),
    );
    r.id = id.to_string();
    r.key = key.map(String::from);
    r
}

fn stamp() -> core::Stamp {
    core::Stamp {
        by: "me".to_string(),
        branch: Some("main".to_string()),
        sha: None,
    }
}

#[test]
fn key_rel_covers_all_five_cases() {
    assert_eq!(key_rel(Some("a"), Some("a")), KeyRel::Same);
    assert_eq!(
        key_rel(Some("n"), Some("o")),
        KeyRel::Differ {
            old: "o".into(),
            new: "n".into()
        }
    );
    assert_eq!(
        key_rel(Some("n"), None),
        KeyRel::OnlyNew { new: "n".into() }
    );
    assert_eq!(
        key_rel(None, Some("o")),
        KeyRel::OnlyOld { old: "o".into() }
    );
    assert_eq!(key_rel(None, None), KeyRel::Neither);
}

#[test]
fn two_unknown_branches_are_not_the_same_branch() {
    assert_eq!(rel_opt(None, None), Rel::Other);
    assert_eq!(rel_opt(Some("main"), None), Rel::Other);
    assert_eq!(rel_opt(Some("main"), Some("main")), Rel::Same);
}

#[test]
fn overlap_keeps_raw_counts() {
    let o = overlap(
        &["a".to_string(), "b".to_string()],
        &["b".to_string(), "c".to_string()],
    );
    assert_eq!(
        o,
        Overlap {
            shared: 1,
            of_new: 2,
            of_old: 2
        }
    );
}

#[test]
fn observe_records_every_dimension() {
    let a = row(
        "01AAAAAAAAAAAAAAAAAAAAAAAAAA",
        "me",
        "note",
        "first pass",
        &["src/a.rs"],
        Some("k:1"),
    );
    let b = row(
        "01BBBBBBBBBBBBBBBBBBBBBBBBBB",
        "other",
        "decision",
        "other words here",
        &["src/b.rs"],
        None,
    );
    let log = core::Log {
        rows: vec![a, b],
        ..Default::default()
    };
    let st = stamp();
    let new = core::Row::new(
        "me",
        "note",
        "see 01BBBBBBBBBBBBBBBBBBBBBBBBBB; second. Supersedes 01AAAAAAAAAAAAAAAAAAAAAAAAAA",
        vec!["src/a.rs".to_string(), "src/c.rs".to_string()],
    );
    let open = open_rows(&log);
    let cands = observe(&log, &open, &st, &new);
    assert_eq!(cands.len(), 2);
    let ea = &cands[0].evidence;
    assert_eq!(ea.named, NameRel::AfterSupersede);
    assert_eq!(ea.writer, Rel::Same);
    assert_eq!(ea.branch, Rel::Other);
    assert_eq!(ea.kind, Rel::Same);
    assert_eq!(ea.key, KeyRel::OnlyOld { old: "k:1".into() });
    assert_eq!(ea.text, TextRel::Differ);
    assert_eq!(ea.files.shared, 1);
    assert_eq!(ea.files.of_new, 2);
    assert_eq!(ea.files.of_old, 1);
    let eb = &cands[1].evidence;
    assert_eq!(eb.named, NameRel::Mentioned);
    assert_eq!(eb.writer, Rel::Other);
    assert_eq!(eb.kind, Rel::Other);
    assert_eq!(eb.key, KeyRel::Neither);
    assert_eq!(eb.text, TextRel::Differ);
    assert_eq!(eb.files.shared, 0);
}
