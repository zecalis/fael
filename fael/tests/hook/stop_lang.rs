//! Stop-hook `[lang]` packs (PLAN-fael-languages chunk 2): the marker packs
//! behind `[lang] marker` decide which phrases block, and `[lang] rows`
//! decides which rows file silently.

use super::{fael, json, repo};

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

fn stop_text(d: &std::path::Path, text: &str) -> (bool, String) {
    let input = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","text":{}}}"#,
        json(d),
        serde_json::Value::String(text.into()),
    );
    let (ok, out, _) = fael(d, &["hook", "stop"], &input);
    (ok, out)
}

#[test]
fn stop_thai_marker_blocks_by_default() {
    let d = repo();
    decide(&d);
    let (ok, out) = stop_text(&d, "เจอบั๊กใน login");
    assert!(ok && out.contains("fael add issue"), "{out}");
}

#[test]
fn stop_marker_english_only_ignores_thai() {
    let d = repo();
    decide(&d);
    lang_config(&d, "[lang]\nmarker = [\"english\"]\n");
    // the Thai pack is off: no block, nothing stashed against a later phrase
    let (ok, out) = stop_text(&d, "เจอบั๊กใน login");
    assert!(ok && out.contains(r#""block":false"#), "{out}");
    // the English pack still fires
    let (ok, out) = stop_text(&d, "I found a bug in login");
    assert!(ok && out.contains("fael add issue"), "{out}");
}

#[test]
fn stop_marker_empty_disables_the_bug_rule() {
    let d = repo();
    decide(&d);
    lang_config(&d, "[lang]\nmarker = []\n");
    let (ok, out) = stop_text(&d, "I found a bug in login");
    assert!(ok && out.contains(r#""block":false"#), "{out}");
}

#[test]
fn stop_thai_risk_note_needs_thai_pack() {
    let d = repo();
    decide(&d);
    lang_config(&d, "[lang]\nmarker = [\"english\"]\n");
    // a Thai risk mention alone never blocks — but with the pack off it must
    // not even stash a line for the next push
    let (ok, out) = stop_text(&d, "doc กับโค้ดไม่ตรงกัน");
    assert!(ok && out.contains(r#""block":false"#), "{out}");
    let read = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","files":["src/nothing.rs"]}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "read"], &read);
    assert!(ok && !out.contains("possible problem"), "{out}");
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
