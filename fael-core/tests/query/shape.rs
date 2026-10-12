//! `add_gate`: the shape faults the caller fixes in the same call reject
//! before the write — a topic list, a long untitled text, a misspelt plan key.

use fael_core::*;

fn row(kind: &str, text: &str, key: Option<&str>) -> Row {
    let mut r = Row::new("tester-0000", kind, text, vec!["src/a.rs".into()]);
    r.key = key.map(String::from);
    r
}

fn gate(r: &Row, log: &Log, old: Option<&str>) -> Result<(), String> {
    add_gate(r, log, &Config::default(), old)
}

#[test]
fn topic_list_and_untitled_reject_before_write() {
    let log = Log::default();
    let list = row("decision", "a · b · c", Some("x:y"));
    let e = gate(&list, &log, None).unwrap_err();
    assert!(e.starts_with("rejected: nothing written"), "{e}");
    assert!(
        e.contains("topic separators") && e.contains("--force"),
        "{e}"
    );
    let long = row("note", &"word ".repeat(70), None);
    let e = gate(&long, &log, None).unwrap_err();
    assert!(e.contains("no title"), "{e}");
    // the reject names the other fat reasons too: one re-run fixes all
    let mut docs = long.clone();
    docs.files = vec!["PLAN-x.md".into()];
    assert!(gate(&docs, &log, None).unwrap_err().contains("plan:x"));
    let mut titled = long;
    titled.title = Some("a headline".into());
    assert!(gate(&titled, &log, None).is_ok());
}

/// `--supersedes <id> --replace` on a row that already was a topic list must
/// go through: the fault is not new, and the re-file is how it gets fixed.
#[test]
fn refile_of_a_fat_row_passes() {
    let mut old = row("decision", "a · b · c", Some("x:y"));
    old.id = "01OLD".into();
    let log = Log {
        rows: vec![old],
        ..Default::default()
    };
    let new = row("decision", "a · b2 · c", Some("x:y"));
    assert!(gate(&new, &log, Some("01OLD")).is_ok());
    assert!(gate(&new, &log, None).is_err());
}
