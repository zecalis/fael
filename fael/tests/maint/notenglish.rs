//! `doctor [NotEnglish]` (PLAN-fael-languages chunk 2): open rows outside the
//! accepted `[lang] rows` scripts report as one batch — full ids in `--json`
//! for the translate pass, and a superseded row leaves the list.

use super::{fael, repo};

/// `add` a row of `kind` on its own file; returns the new id (stdout's first token).
fn add(d: &std::path::Path, kind: &str, text: &str, file: &str) -> String {
    std::fs::write(d.join(file), "").unwrap();
    let (ok, out, err) = fael(d, &["add", kind, text, "--files", file]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

fn notenglish_json(out: &str) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(out).ok()?;
    v.as_array()?
        .iter()
        .find(|p| p["kind"] == "notenglish")
        .cloned()
}

#[test]
fn doctor_notenglish_lists_open_foreign_rows_with_ids() {
    let d = repo();
    add(&d, "decision", "kept choice", "src/a.rs");
    let thai = add(&d, "note", "บันทึกหลัง merge", "src/b.rs");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(out.contains("note [NotEnglish]: 1 open row(s)"), "{out}");
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    let p = notenglish_json(&out).expect("a notenglish entry");
    assert!(
        p["ids"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(&thai))),
        "{out}"
    );
    // a superseded row leaves the list: translate and re-file in English
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "notes after merge",
            "--files",
            "src/b.rs",
            "--supersedes",
            &thai,
        ],
    );
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[NotEnglish]"), "{out}");
    let (_, out, _) = fael(&d, &["doctor", "--json"]);
    assert!(notenglish_json(&out).is_none(), "{out}");
}

#[test]
fn doctor_notenglish_respects_rows_config() {
    let d = repo();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(
        d.join(".fael/config.toml"),
        "[lang]\nrows = [\"english\", \"thai\"]\n",
    )
    .unwrap();
    add(&d, "note", "บันทึกหลัง merge", "src/b.rs");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("[NotEnglish]"), "{out}");
}
