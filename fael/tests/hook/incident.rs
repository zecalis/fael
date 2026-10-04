//! 01M40ARQ step 3: a row a human keys `incident:<kind>` counts once under
//! `incidents` in `fael stats`, per week and kind — `--json` and the text line.

use super::{fael, json, repo};

#[test]
fn incident_keyed_rows_count_per_week_in_stats() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let add = |args: &[&str]| {
        let (ok, out, err) = fael(&d, args, "");
        assert!(ok, "{out}{err}");
    };
    add(&[
        "add",
        "decision",
        "keep the parser pure",
        "--files",
        "src/a.rs",
    ]);
    // a push opens the repo's usage window: rows filed after it count
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&d.join("src/a.rs"))
    );
    let (ok, _, err) = fael(&d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok, "{err}");
    for (text, key) in [
        (
            "two agents both wrote the parser",
            "incident:duplicate-work:parser",
        ),
        (
            "two agents both wrote the lexer",
            "incident:duplicate-work:lexer",
        ),
        ("not an incident", "parser:notes"),
    ] {
        add(&["add", "note", text, "--files", "src/a.rs", "--key", key]);
    }
    let (ok, out, err) = fael(&d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let weeks = v["incidents"].as_object().unwrap();
    assert_eq!(weeks.len(), 1, "{out}");
    let (week, kinds) = weeks.iter().next().unwrap();
    assert_eq!(kinds, &serde_json::json!({"duplicate-work": 2}), "{out}");
    let (ok, text, err) = fael(&d, &["stats"], "");
    assert!(ok, "{err}");
    assert!(
        text.contains(&format!(
            "incidents filed: week of {week}: duplicate-work ×2"
        )),
        "{text}"
    );
}
