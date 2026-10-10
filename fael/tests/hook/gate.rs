//! PLAN-fael-learn-loop chunk 5: `push_policy` names the push gate; a name
//! fael does not know is an error, never silently ignored.

use super::{fael, repo};
use std::path::Path;

fn add(d: &Path, kind: &str, text: &str, file: &str) {
    std::fs::write(d.join(file), "// x\n").unwrap();
    let (ok, _, err) = fael(d, &["add", kind, text, "--files", file], "");
    assert!(ok, "{err}");
}

#[test]
fn an_unknown_policy_is_rejected_not_ignored() {
    let d = repo();
    add(&d, "decision", "keep the parser pure", "src/a.rs");
    add(&d, "issue", "parser loops", "src/a.rs");
    std::fs::write(d.join(".fael/config.toml"), "push_policy = \"touch@9\"\n").unwrap();
    let (_, _, err) = fael(&d, &["find", "parser"], "");
    assert!(err.contains("push_policy"), "{err}");
}
