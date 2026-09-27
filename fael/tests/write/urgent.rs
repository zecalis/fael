//! Urgent queue + bump round trip through the real binary.

use super::{fael, repo, row_json};

#[test]
fn urgent_and_bump_round_trip() {
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
    // bump the second above the first: half the top → 0.5, only it rewritten
    let id_b = row_json(&d, "second hot")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let id_a = row_json(&d, "first hot")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (ok, out, err) = fael(&d, &["bump", &id_b, "--urgent-before", &id_a], "");
    assert!(ok, "{err}");
    let id_b2 = out.split_whitespace().next().unwrap().to_string();
    assert_ne!(id_b, id_b2);
    let b2 = row_json(&d, "second hot");
    assert_eq!(b2["urgent"].as_f64(), Some(0.5));
    assert_eq!(b2["id"].as_str().unwrap(), id_b2);
    assert_eq!(b2["supersedes"].as_str().unwrap(), id_b);
    // the queue order follows the new number, rendered on the line
    let (_, out, _) = fael(&d, &["find", "--kind", "issue"], "");
    assert!(out.contains("second hot (urgent 0.5)"), "{out}");
    assert!(
        out.find("second hot").unwrap() < out.find("first hot").unwrap(),
        "{out}"
    );
    // --not-urgent leaves the queue, --to routes (lowercased)
    let (ok, _, err) = fael(&d, &["bump", &id_b2, "--not-urgent", "--to", "Ploy"], "");
    assert!(ok, "{err}");
    let b3 = row_json(&d, "second hot");
    assert!(b3.get("urgent").is_none(), "{b3}");
    assert_eq!(b3["to"].as_str().unwrap(), "ploy");
}
