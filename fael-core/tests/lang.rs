//! `[lang]` config + core language packs (PLAN-fael-languages chunk 1):
//! the registry, the Stop-hook matcher and the row-language warning.

use fael_core::{Config, by_name, marker_hit, row_language_check};

fn en_th() -> Vec<&'static fael_core::Lang> {
    vec![by_name("english").unwrap(), by_name("thai").unwrap()]
}

fn en_only() -> Vec<&'static fael_core::Lang> {
    vec![by_name("english").unwrap()]
}

fn cfg(rows: &[&str]) -> Config {
    Config {
        lang_rows: rows.iter().map(|s| s.to_string()).collect(),
        ..Config::default()
    }
}

#[test]
fn lang_defaults_keep_old_behaviour() {
    let c = Config::default();
    assert_eq!(c.lang_marker, ["english", "thai"]);
    assert_eq!(c.lang_rows, ["english"]);
    let c = Config::from_toml("").unwrap();
    assert_eq!(c.lang_marker, ["english", "thai"]);
    assert_eq!(c.lang_rows, ["english"]);
}

#[test]
fn lang_section_keys_are_each_optional() {
    let c = Config::from_toml("[lang]\nmarker = [\"english\"]").unwrap();
    assert_eq!(c.lang_marker, ["english"]);
    assert_eq!(c.lang_rows, ["english"]);
    let c = Config::from_toml("[lang]\nrows = [\"english\", \"thai\"]").unwrap();
    assert_eq!(c.lang_marker, ["english", "thai"]);
    assert_eq!(c.lang_rows, ["english", "thai"]);
}

#[test]
fn unknown_pack_names_reject_like_store() {
    for (key, toml) in [
        ("marker", "[lang]\nmarker = [\"german\"]"),
        ("rows", "[lang]\nrows = [\"english\", \"german\"]"),
    ] {
        let e = Config::from_toml(toml).unwrap_err();
        assert!(e.contains("[lang]") && e.contains(key), "{e}");
        assert!(e.contains("german") && e.contains("english|thai"), "{e}");
    }
    assert!(by_name("german").is_none());
}

#[test]
fn empty_marker_switches_the_bug_rule_off() {
    let empty: Vec<&fael_core::Lang> = vec![];
    for text in [
        "I found a bug in login",
        "เจอบั๊กที่ login",
        "config and schema are out of sync",
        "Bug (cause):",
    ] {
        assert!(marker_hit(text, &empty, &[]).is_none(), "{text}");
    }
}

#[test]
fn english_only_pack_ignores_thai_phrases() {
    let packs = en_only();
    assert!(marker_hit("I found a bug in login", &packs, &[]).is_some());
    assert!(marker_hit("config and schema are out of sync", &packs, &[]).is_some());
    for thai in ["เจอบั๊กที่ login", "doc กับโค้ดไม่ตรงกัน", "ตรงนี้อาจมีปัญหา"]
    {
        assert!(marker_hit(thai, &packs, &[]).is_none(), "{thai}");
    }
}

#[test]
fn default_packs_catch_both_languages() {
    let packs = en_th();
    let hit = marker_hit("I found a bug in login", &packs, &[]).unwrap();
    assert!(hit.strong, "{}", hit.marker);
    let hit = marker_hit("เจอบั๊กที่ login", &packs, &[]).unwrap();
    assert!(hit.strong, "{}", hit.marker);
    let hit = marker_hit("doc กับโค้ดไม่ตรงกัน", &packs, &[]).unwrap();
    assert!(!hit.strong, "{}", hit.marker);
}

#[test]
fn markers_catch_bugs_and_risks_not_denials() {
    let packs = en_th();
    for hit in [
        "I found a bug in login",
        "doc กับโค้ดไม่ตรงกัน",
        "config and schema are out of sync",
        "this might break the importer",
        "ตรงนี้อาจมีปัญหาตอน merge",
        "Bug (cause):",
    ] {
        assert!(marker_hit(hit, &packs, &[]).is_some(), "{hit}");
    }
    for miss in [
        "no bug found",
        "no mismatch left",
        "ไม่มีความเสี่ยง",
        "ถ้าเจอบั๊กให้บอก",
        "all tests pass",
    ] {
        assert!(marker_hit(miss, &packs, &[]).is_none(), "{miss}");
    }
}

#[test]
fn quoted_code_never_signals() {
    let packs = en_th();
    for quiet in [
        "```\nI found a bug in login\n```",
        "run `found a bug` to reproduce",
        "> I found a bug in login",
        "> doc กับโค้ดไม่ตรงกัน",
        "```\nconfig and schema are out of sync\n```",
    ] {
        assert!(marker_hit(quiet, &packs, &[]).is_none(), "{quiet}");
    }
    // prose around code still fires
    let hit = marker_hit(
        "looks off:\n```\nlet x = 1;\n```\nI found a bug below",
        &packs,
        &[],
    )
    .unwrap();
    assert!(hit.strong, "{}", hit.marker);
}

/// A risk inside a conditional is a hypothetical (2026-10-01: "ถ้าทำพร้อมกัน
/// … ตัวเลขไม่ตรงกัน" nudged an issue on files it never named). The
/// conditional covers its own sentence only.
#[test]
fn conditional_risks_stay_quiet() {
    let packs = en_th();
    for quiet in [
        "ถ้าทำพร้อมกันจะได้นิยามสองแบบที่ตัวเลขไม่ตรงกัน",
        "หากรันสองที่ ค่าอาจมีปัญหา",
        "if both run at once the counts mismatch",
    ] {
        assert!(marker_hit(quiet, &packs, &[]).is_none(), "{quiet}");
    }
    for hit in [
        "if needed, rerun. the counts mismatch",
        "the verify step shows a mismatch",
        "ถ้าว่างค่อยดู\ndoc กับโค้ดไม่ตรงกัน",
    ] {
        assert!(marker_hit(hit, &packs, &[]).is_some(), "{hit}");
    }
}

#[test]
fn negations_extra_cancels_like_a_builtin() {
    let packs = en_th();
    assert!(marker_hit("x found a bug", &packs, &[]).is_some());
    assert!(marker_hit("x found a bug", &packs, &["x"]).is_none());
}

#[test]
fn default_rows_warn_byte_identical() {
    let c = cfg(&["english"]);
    assert_eq!(
        row_language_check(&c, Some("หัวข้อไทย"), "stale notes"),
        Some("row not in English — write rows in English from now on".into())
    );
    assert_eq!(
        row_language_check(&c, None, "stale notes หัวข้อไทย"),
        Some("row not in English — write rows in English from now on".into())
    );
    assert_eq!(
        row_language_check(&c, None, "stale notes after merge"),
        None
    );
    assert_eq!(
        row_language_check(&c, None, "a → b when ≤ 3, café laté"),
        None
    );
    // a term quoted in backticks is cited verbatim, not the row's language
    assert_eq!(
        row_language_check(&c, Some("rename `ภาษี` field"), "the `ภาษี` label stays"),
        None
    );
}

#[test]
fn empty_rows_switches_the_warning_off() {
    // `rows = []` mirrors `marker = []`: no accepted script would otherwise
    // make every letter foreign and garble the message
    let c = cfg(&[]);
    assert_eq!(row_language_check(&c, Some("หัวข้อไทย"), "stale notes"), None);
    assert_eq!(row_language_check(&c, None, "stale notes"), None);
}

#[test]
fn thai_rows_pass_when_allowed_cjk_still_warns() {
    let c = cfg(&["english", "thai"]);
    assert_eq!(
        row_language_check(&c, Some("หัวข้อไทย"), "stale notes หัวข้อไทย"),
        None
    );
    let w = row_language_check(&c, None, "stale notes バグ").unwrap();
    assert!(w.contains("english/thai"), "{w}");
    // title counts, same as before
    assert!(row_language_check(&c, Some("バグ"), "plain text").is_some());
}
