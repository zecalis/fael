//! Urgent queue through the real binary (bump --why still records a reason).

use super::{fael, repo, row_json};

#[test]
fn urgent_files_at_the_back_of_the_queue() {
    let d = repo();
    // --urgent files at the back of the queue: 1.0, then 2.0
    for text in ["first hot", "second hot"] {
        let (ok, _, err) = fael(
            &d,
            &["add", "issue", text, "--files", "doc:a", "--urgent"],
            "",
        );
        assert!(ok, "{err}");
    }
    assert_eq!(row_json(&d, "first hot")["urgent"].as_f64(), Some(1.0));
    assert_eq!(row_json(&d, "second hot")["urgent"].as_f64(), Some(2.0));
    // --urgent on a decision is rejected: the queue holds issues
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "not hot", "--files", "doc:a", "--urgent"],
        "",
    );
    assert!(!ok && err.contains("urgent is for issues"), "{err}");
    // the queue order follows the number, rendered on the line
    let (_, out, _) = fael(&d, &["find", "--kind", "issue"], "");
    assert!(out.contains("first hot (urgent 1)"), "{out}");
    assert!(out.contains("second hot (urgent 2)"), "{out}");
    assert!(
        out.find("first hot").unwrap() < out.find("second hot").unwrap(),
        "{out}"
    );
}

#[test]
fn bump_why_is_kept_in_the_event() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "note", "gate scope", "--files", "doc:a"], "");
    assert!(ok, "{err}");
    let id = row_json(&d, "gate scope")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (ok, _, err) = fael(&d, &["bump", &id, "--why", "unrelated edit"], "");
    assert!(ok, "{err}");
    // the fold hides the event, so read the raw log for the reason
    let raw = std::process::Command::new("grep")
        .args(["-rh", "bumped: unrelated edit"])
        .arg(&d)
        .output()
        .unwrap();
    assert!(!raw.stdout.is_empty(), "bump event lost its reason");
}
