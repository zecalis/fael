//! `doctor` for log health: union setup, gone/part-gone notes, stale
//! backtick pointers, and quarantine of broken lines.

use super::{fael, repo};
use std::path::PathBuf;

#[test]
fn doctor_fails_without_union_then_fix_repairs() {
    let d = repo();
    // no log yet: info only, exit 0
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok && out.contains("1 problem(s)"), "{out}");
    // adopt fael without the union line: error, exit 1
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, _, _) = fael(&d, &["add", "note", "first row", "--files", "src/a.rs"]);
    assert!(ok);
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(!ok && out.contains("error"), "{out}");
    let (ok, out, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    assert!(out.contains("merge=union"), "{out}");
    assert!(d.join(".gitattributes").exists());
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok && out.contains("clean"), "{out}");
    // excluded locally on purpose: a note, not an error; in .gitignore: an error
    std::fs::write(d.join(".git/info/exclude"), ".fael/log/\n").unwrap();
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok && out.contains("note [Ignored]"), "{out}");
    std::fs::write(d.join(".gitignore"), ".fael/log/\n").unwrap();
    std::fs::write(d.join(".git/info/exclude"), "").unwrap();
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(!ok && out.contains("error [Ignored]"), "{out}");
    // an open row on a deleted file is reported, never an error
    fael(
        &d,
        &[
            "add",
            "issue",
            "on a gone file",
            "--files",
            "src/deleted.rs",
        ],
    );
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Gone]: 1 open row(s)") && out.contains("src/deleted.rs"),
        "{out}"
    );
    // one file left, one gone: still pushes, reported apart, naming only the gone file
    fael(
        &d,
        &[
            "add",
            "issue",
            "half gone",
            "--files",
            "src/a.rs,src/removed.rs",
            "--force",
        ],
    );
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Gone]: 1 open row(s)")
            && out.contains("note [PartGone]: 1 open row(s)")
            && out.contains("→ src/removed.rs"),
        "{out}"
    );
}

#[test]
fn doctor_flags_stale_backtick_paths() {
    let d = repo();
    std::fs::write(d.join("keep.yaml"), "").unwrap();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    // the row files a live file; the backticked pointer is prose, not files[]
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "catalog ใน `keep.yaml`",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Stale]"), "{out}");
    // the file goes away, the pointer stays: Stale names the row id
    std::fs::remove_file(d.join("keep.yaml")).unwrap();
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Stale]: 1 open row(s)") && out.contains("→ keep.yaml"),
        "{out}"
    );
    // talk in backticks (no path) never flags
    fael(
        &d,
        &["add", "note", "run `merge=union` after", "--files", "src"],
    );
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Stale]: 1 open row(s)") && !out.contains("merge=union"),
        "{out}"
    );
}

#[test]
fn doctor_json_lists_full_row_ids() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "on a gone file",
            "--files",
            "src/deleted.rs",
        ],
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    // `--json` carries the full id (not just the abbreviated example in detail)
    // (exit may be 1 — no union line yet is an unrelated error)
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let ps: Vec<serde_json::Value> = serde_json::from_str(out.trim()).unwrap();
    let gone = ps.iter().find(|p| p["kind"] == "gone").expect("gone");
    assert_eq!(gone["ids"], serde_json::json!([id]), "{gone}");
    // the id is actionable: closing it clears the problem
    let (ok, _, err) = fael(&d, &["close", &id, "moved"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    assert!(!out.contains("\"gone\""), "{out}");
}

#[test]
fn doctor_quarantines_a_broken_line() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "decision", "keep me", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let wdir = std::fs::read_dir(d.join(".fael/log"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let log: Vec<PathBuf> = std::fs::read_dir(&wdir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(log.len(), 1);
    let mut s = std::fs::read_to_string(&log[0]).unwrap();
    s.push_str("not json\n");
    std::fs::write(&log[0], s).unwrap();
    let (ok, _, _) = fael(&d, &["doctor"]);
    assert!(!ok);
    let (ok, out, _) = fael(&d, &["doctor", "--fix", "--json"]);
    assert!(ok, "{out}");
    let q: Vec<_> = std::fs::read_dir(d.join(".fael/quarantine"))
        .unwrap()
        .collect();
    assert_eq!(q.len(), 1);
    let (_, out, _) = fael(&d, &["find", "--all"]);
    assert!(out.contains("keep me"), "{out}");
}
