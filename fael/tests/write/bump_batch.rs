//! Batch bump: several ids ride one `fael bump` call bare; a bad id reports
//! alone while the rest save; field flags still take one id.

use super::{fael, repo, row_json};

fn id(d: &std::path::Path, needle: &str) -> String {
    row_json(d, needle)["id"].as_str().unwrap().to_string()
}

#[test]
fn several_ids_ride_one_call_bare() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "b\n").unwrap();
    assert!(fael(&d, &["add", "note", "batch-one", "--files", "src/a.rs"], "").0);
    assert!(fael(&d, &["add", "note", "batch-two", "--files", "src/b.rs"], "").0);
    let (a, b) = (id(&d, "batch-one"), id(&d, "batch-two"));
    let (ok, out, err) = fael(&d, &["bump", &a, &b], "");
    assert!(ok, "{out} {err}");
    assert!(out.contains(&a) && out.contains(&b), "{out}");
    // a bad id reports alone while the rest save
    let (ok, out, _) = fael(&d, &["bump", &a, "01NOPE"], "");
    assert!(!ok, "{out}");
    assert!(out.contains("rejected: 01NOPE"), "{out}");
    assert!(out.contains(&a), "{out}");
    // field flags move one row — several ids with any of them is rejected
    let (ok, _, err) = fael(&d, &["bump", &a, &b, "--to", "ploy"], "");
    assert!(!ok && err.contains("one row"), "{err}");
    let (ok, _, err) = fael(&d, &["bump", &a, &b, "--urgent"], "");
    assert!(!ok && err.contains("one row"), "{err}");
}
