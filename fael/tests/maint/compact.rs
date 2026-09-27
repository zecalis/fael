//! `compact` round trip through the real binary: past months fold their
//! closes, and folded rows hide by default but list with `--all`.

use super::{fael, repo};

#[test]
fn compact_round_trip_through_cli() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "note", "current row", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    // a past month with a close, written by hand (the CLI only writes this month)
    let by = std::fs::read_dir(d.join(".fael/log"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name();
    let dir = d.join(".fael/log").join(by);
    let row = |id: &str, text: &str| {
        format!(
            r#"{{"v":1,"id":"{id}","ts":"2000-01-01T00:00:00.000Z","by":"test-user-","kind":"decision","text":"{text}","files":["old.rs"]}}"#
        )
    };
    std::fs::write(
        dir.join("2000-01.jsonl"),
        format!(
            "{}\n{}\n",
            row("A0000000000000000000000001", "old one"),
            row("A0000000000000000000000002", "old two")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("2000-01.close.jsonl"),
        "{\"v\":1,\"id\":\"C0000000000000000000000001\",\"ts\":\"2000-01-02T00:00:00.000Z\",\"by\":\"test-user-\",\"ref\":\"A0000000000000000000000001\",\"text\":\"done\"}\n",
    )
    .unwrap();
    let (ok, out, err) = fael(&d, &["compact"]);
    assert!(ok, "{err}");
    assert!(out.contains("1 close(s) folded"), "{out}");
    assert!(!dir.join("2000-01.jsonl").exists());
    let (_, out, _) = fael(&d, &["find", "--all"]);
    assert!(
        out.contains("old one") && out.contains("old two") && out.contains("current row"),
        "{out}"
    );
    let (_, out, _) = fael(&d, &["find"]);
    assert!(
        !out.contains("old one") && out.contains("current row"),
        "{out}"
    ); // folded close hides by default
}
