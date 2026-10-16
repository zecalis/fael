//! Chunk 3b: repeated notes file beside each other; ambiguity files and lists, never asks.

use super::{fael, names, open_notes, repo, usage};

fn add(d: &std::path::Path, text: &str, files: &str) -> (bool, String, String) {
    fael(d, &["add", "note", text, "--files", files], "")
}

#[test]
fn second_overlapping_note_files_beside_first() {
    let d = repo();
    let (ok, _, err) = add(&d, "first pass", "src/a.rs");
    assert!(ok, "{err}");
    let first = open_notes(&d);
    assert_eq!(first.len(), 1);
    let (ok, _, err) = add(&d, "second pass", "src/a.rs,src/b.rs");
    assert!(ok, "{err}");
    // shared files prove relatedness, not replacement: file the row and name
    // what stays open — one info line, no ask counted
    assert!(!err.contains("superseded"), "{err}");
    assert!(err.contains("kept all"), "{err}");
    assert!(names(&err, "open notes ", &first[0]), "{err}");
    let open = open_notes(&d);
    assert_eq!(open.len(), 2);
    assert!(open.contains(&first[0]));
    let (ok, out, err) = fael(&d, &["find", "--json", "--all"], "");
    assert!(ok, "{err}");
    let yours = out
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["id"].as_str() == Some(open[1].as_str()))
        .unwrap();
    assert!(yours["supersedes"].is_null(), "{yours}");
    assert!(yours["decision_source"].is_null(), "{yours}");
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}

#[test]
fn several_open_notes_file_and_list_without_asking() {
    let d = repo();
    let (ok, _, err) = add(&d, "about a", "src/a.rs");
    assert!(ok, "{err}");
    let (ok, _, err) = add(&d, "about b", "src/b.rs");
    assert!(ok, "{err}");
    // spanning both: two candidates — ambiguous, so file it, supersede
    // nothing, name them in one info line; a reject here would have no way out
    let (ok, _, err) = add(&d, "about both", "src/a.rs,src/b.rs");
    assert!(ok, "{err}");
    assert!(
        err.contains("open notes") && err.contains("--supersedes"),
        "{err}"
    );
    assert_eq!(open_notes(&d).len(), 3, "nothing superseded");
    assert!(usage(&d).is_empty(), "the list is info, no ask");
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
    // the files hold is notes-only: decisions never auto-supersede by files
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
