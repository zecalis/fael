//! `doctor [NoVerdict]`: open rows that name a real file over the push cap
//! (1 MiB). `fael add` stamps files up to 16 MiB, but the edit push compares
//! only up to the cap, so such a row gets the generic hint forever — the note
//! makes that miss visible per row. Read-only: `--fix` never touches it.

use super::{fael, repo};
use std::path::Path;

const MIB: usize = 1024 * 1024;

fn add(d: &Path, text: &str, file: &str) -> String {
    let (ok, out, err) = fael(d, &["add", "decision", text, "--files", file]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

fn big(d: &Path, name: &str, bytes: usize) {
    std::fs::write(d.join(name), vec![b'x'; bytes]).unwrap();
}

fn json(d: &Path) -> Vec<serde_json::Value> {
    let (_, out, _) = fael(d, &["doctor", "--json"]);
    serde_json::from_str(out.trim()).unwrap()
}

fn kind<'a>(ps: &'a [serde_json::Value], k: &str) -> Option<&'a serde_json::Value> {
    ps.iter().find(|p| p["kind"] == k)
}

#[test]
fn a_row_on_a_file_over_the_push_cap_is_listed() {
    let d = repo();
    big(&d, "src/big.bin", 2 * MIB);
    let id = add(&d, "big asset choice", "src/big.bin");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [NoVerdict]: 1 open row(s)")
            && out.contains(&format!("{} → src/big.bin (2.0 MiB)", &id[..8]))
            && out.contains("generic hint stands"),
        "{out}"
    );
    assert!(
        !out.contains("[PartGone]") && !out.contains("[Gone]"),
        "{out}"
    );
}

#[test]
fn a_file_at_or_under_the_cap_is_not_listed() {
    let d = repo();
    big(&d, "src/small.bin", 10);
    big(&d, "src/edge.bin", MIB); // exactly the cap still has a verdict
    add(&d, "small", "src/small.bin");
    add(&d, "edge", "src/edge.bin");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[NoVerdict]"), "{out}");
}

#[test]
fn a_missing_file_is_part_gone_only() {
    let d = repo();
    big(&d, "src/big.bin", 2 * MIB);
    big(&d, "src/small.bin", 10);
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "big and a small one",
            "--files",
            "src/big.bin",
            "--files",
            "src/small.bin",
        ],
    );
    assert!(ok, "{err}");
    std::fs::remove_file(d.join("src/big.bin")).unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(out.contains("[PartGone]"), "{out}");
    assert!(!out.contains("[NoVerdict]"), "{out}");
}

#[test]
fn anchors_and_directories_are_skipped() {
    let d = repo();
    std::fs::create_dir_all(d.join("src/dir.bin")).unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "anchored",
            "--files",
            "src/dir.bin",
            "--files",
            "doc:pricing",
        ],
    );
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[NoVerdict]"), "{out}");
}

#[test]
fn a_closed_row_is_not_listed() {
    let d = repo();
    big(&d, "src/big.bin", 2 * MIB);
    let id = add(&d, "big asset choice", "src/big.bin");
    let (ok, _, err) = fael(&d, &["close", &id, "done"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[NoVerdict]"), "{out}");
}

#[test]
fn json_carries_the_full_id() {
    let d = repo();
    big(&d, "src/big.bin", 2 * MIB);
    let id = add(&d, "big asset choice", "src/big.bin");
    let ps = json(&d);
    let nv = kind(&ps, "noverdict").expect("noverdict");
    assert_eq!(nv["severity"], "info", "{nv}");
    assert_eq!(nv["fixable"], false, "{nv}");
    assert_eq!(nv["ids"], serde_json::json!([id]), "{nv}");
}

#[test]
fn fix_leaves_the_row_open_and_the_note_in_place() {
    let d = repo();
    big(&d, "src/big.bin", 2 * MIB);
    let id = add(&d, "big asset choice", "src/big.bin");
    let (ok, _, err) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["find", "--files", "src/big.bin"]);
    assert!(out.contains("big asset choice"), "{out}");
    let ps = json(&d);
    let nv = kind(&ps, "noverdict").expect("still listed after --fix");
    assert_eq!(nv["ids"], serde_json::json!([id]), "{nv}");
}
