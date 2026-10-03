//! Stop-hook `[lang]` packs (PLAN-fael-languages chunk 2): the marker packs
//! behind `[lang] marker` decide which phrases are flagged, and `[lang] rows`
//! decides which rows file silently.

use super::{fael, flagged, json, repo};

const SESSION: &str = r#""2020-01-01T00:00:00Z""#;

fn decide(d: &std::path::Path) {
    let (ok, _, err) = fael(
        d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
}

fn lang_config(d: &std::path::Path, body: &str) {
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), body).unwrap();
}

/// Stop with `text`, then whether the next push flags it.
fn stop_flags(d: &std::path::Path, text: &str) -> bool {
    let input = format!(
        r#"{{"cwd":{},"session":{SESSION},"text":{}}}"#,
        json(d),
        serde_json::Value::String(text.into()),
    );
    let (ok, out, _) = fael(d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":false"#), "{out}");
    flagged(d, SESSION)
}

#[test]
fn stop_thai_marker_flags_by_default() {
    let d = repo();
    decide(&d);
    assert!(stop_flags(&d, "เจอบั๊กใน login"));
}

#[test]
fn stop_marker_english_only_ignores_thai() {
    let d = repo();
    decide(&d);
    lang_config(&d, "store = \"tracked\"\n[lang]\nmarker = [\"english\"]\n");
    // the Thai pack is off: neither a bug nor a risk phrase is flagged
    assert!(!stop_flags(&d, "เจอบั๊กใน login"));
    assert!(!stop_flags(&d, "doc กับโค้ดไม่ตรงกัน"));
    // the English pack still fires
    assert!(stop_flags(&d, "I found a bug in login"));
}

#[test]
fn stop_marker_empty_disables_the_bug_rule() {
    let d = repo();
    decide(&d);
    lang_config(&d, "store = \"tracked\"\n[lang]\nmarker = []\n");
    assert!(!stop_flags(&d, "I found a bug in login"));
}

#[test]
fn add_rows_thai_accepted_silently() {
    let d = repo();
    lang_config(&d, "[lang]\nrows = [\"english\", \"thai\"]\n");
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "บันทึกหลัง merge", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("not in English"), "{err}");
}
