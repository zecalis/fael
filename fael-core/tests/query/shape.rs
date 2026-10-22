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

/// `close_gate`: a bad close is permanent (closing twice is already-closed),
/// so like `add` it rejects before the write. An issue owes a guard or a
/// don't-because; a note owes the arrow only.
#[test]
fn close_gate_needs_core_and_a_check_for_issues() {
    let bad = close_gate("fixed it", "issue").unwrap_err();
    assert!(bad.starts_with("rejected: nothing written"), "{bad}");
    assert!(bad.contains("no fix") && bad.contains("--force"), "{bad}");
    let noguard = close_gate("slow query → added index", "issue").unwrap_err();
    assert!(noguard.contains("no check"), "{noguard}");
    assert!(
        close_gate(
            "slow query → added index; guard `db/index_test.rs`",
            "issue"
        )
        .is_ok()
    );
    assert!(
        close_gate(
            "slow query → added index; don't add a covering index because writes double",
            "issue"
        )
        .is_ok()
    );
    // a note owes nothing: a done handoff, a held verdict are complete as written
    assert!(close_gate("handoff done", "note").is_ok());
    assert!(close_gate("now in `src/a.rs`", "decision").is_ok());
    // an arrow inside a code span alone is no fix
    assert!(close_gate("`a -> b` fixed", "issue").is_err());
    // the full template shape passes: `;` clauses are the template, not a topic list
    assert!(
        close_gate(
            "slow query → added an index; guard `db/index_test.rs`; tried vacuuming first",
            "issue"
        )
        .is_ok()
    );
}
