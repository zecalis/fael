//! The edit ask across sessions (src/hook/asked.rs): said once until the row
//! changes. Every real edit moves the file's bytes, so bytes never decide.

use super::changed_hint::{add, edit_or_silent};
use super::{fael, repo};

fn asked(out: &str) -> bool {
    out.contains("changed since")
}

/// Asked in A; silent in B though the bytes moved again; a bump makes C ask.
#[test]
fn ask_is_said_once_until_the_row_is_bumped() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "issue", "retry uses backoff here", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2\n").unwrap();
    let a = edit_or_silent(&d, "sA", "src/a.rs");
    assert!(asked(&a), "{a}");

    std::fs::write(d.join("src/a.rs"), "// v3 longer\n").unwrap();
    let b = edit_or_silent(&d, "sB", "src/a.rs");
    assert!(!asked(&b) && !b.contains("fael close"), "{b}");

    let (ok, _, err) = fael(&d, &["bump", &id, "--to", "owner"], "");
    assert!(ok, "{err}");
    std::fs::write(d.join("src/a.rs"), "// v4 longer still\n").unwrap();
    let c = edit_or_silent(&d, "sC", "src/a.rs");
    assert!(asked(&c), "{c}");

    // and once more silent: the bump was asked about
    std::fs::write(d.join("src/a.rs"), "// v5\n").unwrap();
    let e = edit_or_silent(&d, "sD", "src/a.rs");
    assert!(!asked(&e), "{e}");
}
