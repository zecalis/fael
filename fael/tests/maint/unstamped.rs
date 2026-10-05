//! `doctor [Unstamped]`: open rows with no `fh` on a real file push could
//! compare. A row filed before its file existed has no stamp, so push never
//! gives it a verdict; a bare `fael bump <id>` restamps it. Read-only: `--fix`
//! never touches it.

use super::{fael, repo};
use std::path::Path;

/// A row on a file that is not there yet, so it is written without `fh`.
fn unstamped(d: &Path, file: &str) -> String {
    let (ok, out, err) = fael(d, &["add", "decision", "later file", "--files", file]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

#[test]
fn a_row_filed_before_its_file_existed_is_listed_until_bumped() {
    let d = repo();
    let id = unstamped(&d, "src/later.rs");
    std::fs::write(d.join("src/later.rs"), "fn main() {}\n").unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Unstamped]: 1 open row(s)")
            && out.contains(&format!("{} → src/later.rs", &id[..8]))
            && out.contains("fael bump"),
        "{out}"
    );
    let (ok, _, err) = fael(&d, &["bump", &id]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Unstamped]"), "{out}");
}

#[test]
fn a_row_stamped_at_add_is_not_listed() {
    let d = repo();
    std::fs::write(d.join("src/now.rs"), "fn main() {}\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "now", "--files", "src/now.rs"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Unstamped]"), "{out}");
}

#[test]
fn nothing_to_stamp_is_not_listed() {
    let d = repo();
    // missing file (PartGone's), an anchor, an over-cap file (NoVerdict's)
    unstamped(&d, "src/never.rs");
    let (ok, _, err) = fael(&d, &["add", "note", "anchored", "--files", "doc:pricing"]);
    assert!(ok, "{err}");
    unstamped(&d, "src/big.bin");
    std::fs::write(d.join("src/big.bin"), vec![b'x'; 2 * 1024 * 1024]).unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Unstamped]"), "{out}");
}

#[test]
fn a_closed_row_is_not_listed_and_json_carries_the_full_id() {
    let d = repo();
    let open = unstamped(&d, "src/a.rs");
    let closed = unstamped(&d, "src/b.rs");
    std::fs::write(d.join("src/a.rs"), "a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "b\n").unwrap();
    let (ok, _, err) = fael(&d, &["close", &closed, "done"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps: Vec<serde_json::Value> = serde_json::from_str(out.trim()).unwrap();
    let p = ps.iter().find(|p| p["kind"] == "unstamped").expect(&out);
    assert_eq!(p["ids"], serde_json::json!([open]), "{out}");
}
