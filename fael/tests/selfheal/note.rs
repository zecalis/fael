//! Chunk 3b: repeated notes self-supersede; ambiguity asks with the list.

use super::{fael, open_notes, repo, usage};

fn add(d: &std::path::Path, text: &str, files: &str) -> (bool, String, String) {
    fael(d, &["add", "note", text, "--files", files], "")
}

#[test]
fn second_overlapping_note_supersedes_first() {
    let d = repo();
    let (ok, _, err) = add(&d, "first pass", "src/a.rs");
    assert!(ok, "{err}");
    let first = open_notes(&d);
    assert_eq!(first.len(), 1);
    let (ok, _, err) = add(&d, "second pass", "src/a.rs,src/b.rs");
    assert!(ok, "{err}");
    // the choice is reported, not asked: one info line, no ask counted
    assert!(err.contains(&format!("superseded {}", first[0])), "{err}");
    let open = open_notes(&d);
    assert_eq!(open.len(), 1);
    assert_ne!(open[0], first[0]);
    let (ok, out, err) = fael(&d, &["find", "--json", "--all"], "");
    assert!(ok, "{err}");
    let yours = out
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["id"].as_str() == Some(open[0].as_str()))
        .unwrap();
    assert_eq!(yours["supersedes"].as_str(), Some(first[0].as_str()));
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}

#[test]
fn several_open_notes_ask_with_the_list() {
    let d = repo();
    let (ok, _, err) = add(&d, "about a", "src/a.rs");
    assert!(ok, "{err}");
    let (ok, _, err) = add(&d, "about b", "src/b.rs");
    assert!(ok, "{err}");
    // spanning both: two candidates — genuinely ambiguous, ask with the list
    let (ok, _, err) = add(&d, "about both", "src/a.rs,src/b.rs");
    assert!(!ok && err.contains("rejected: open notes"), "{err}");
    assert!(err.contains("--supersedes"), "{err}");
    let open = open_notes(&d);
    assert_eq!(open.len(), 2, "the ambiguous add files nothing");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "about both",
            "--files",
            "src/a.rs,src/b.rs",
            "--supersedes",
            &open[0],
        ],
        "",
    );
    assert!(ok, "{err}");
    assert_eq!(open_notes(&d).len(), 2);
}

#[test]
fn other_writer_stays_untouched() {
    let d = repo();
    let (ok, _, err) = add(&d, "first pass", "src/a.rs");
    assert!(ok, "{err}");
    // another writer's note on the same files is not mine to close
    assert!(
        std::process::Command::new("git")
            .args(["config", "user.name", "Someone Else"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let (ok, _, err) = add(&d, "other writer", "src/a.rs");
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert_eq!(open_notes(&d).len(), 2);
}

#[test]
fn other_branch_stays_untouched() {
    let d = repo();
    let (ok, _, err) = add(&d, "first pass", "src/a.rs");
    assert!(ok, "{err}");
    assert!(
        std::process::Command::new("git")
            .args(["checkout", "-qb", "feature"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let (ok, _, err) = add(&d, "branched note", "src/a.rs");
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
}

#[test]
fn disjoint_files_and_other_kinds_stay_untouched() {
    let d = repo();
    let (ok, _, err) = add(&d, "about a", "src/a.rs");
    assert!(ok, "{err}");
    // no file in common: a different topic, not a repeat
    let (ok, _, err) = add(&d, "about b", "src/b.rs");
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    // (b) is notes-only: decisions never auto-supersede by files
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "stance",
            "--files",
            "src/a.rs",
            "--key",
            "test:stance",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert_eq!(open_notes(&d).len(), 2);
}

#[test]
fn explicit_supersedes_passes_through() {
    let d = repo();
    let (ok, _, err) = add(&d, "first pass", "src/a.rs");
    assert!(ok, "{err}");
    let first = open_notes(&d);
    // the agent was specific: no self-heal info, the flag does the work
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "second pass",
            "--files",
            "src/b.rs",
            "--supersedes",
            &first[0],
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert_eq!(open_notes(&d).len(), 1);
}
