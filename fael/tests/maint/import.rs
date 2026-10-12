//! `import` through the real binary: another clone's log in without a dropped
//! row, a close folded into its target, and `--map` subtree moves.

use super::{fael, repo};

#[test]
fn import_a_fael_log_drops_nothing() {
    let d = repo();
    let log = d.join("other").join(".fael").join("log");
    std::fs::create_dir_all(&log).unwrap();
    let lines = [
        r#"{"v":1,"ts":"2026-01-01T00:00:00Z","by":"delamind","id":"01AAAAAAAAAAAAAAAAAAAAAAA1","kind":"decision","text":"picked x","files":["src/a.rs"]}"#,
        r#"{"v":1,"ts":"2026-01-02T00:00:00Z","by":"delamind","id":"01AAAAAAAAAAAAAAAAAAAAAAA2","kind":"issue","text":"login loops","files":["src/b.rs"]}"#,
        r#"{"v":1,"ts":"2026-01-03T00:00:00Z","by":"delamind","kind":"note","text":"no id","files":["src/c.rs"]}"#,
        r#"{"v":1,"ts":"2026-01-04T00:00:00Z","by":"delamind","id":"01AAAAAAAAAAAAAAAAAAAAAAA4","ref":"01AAAAAAAAAAAAAAAAAAAAAAA1","text":"done","files":[]}"#,
        r#"{"ts":"2026-01-05T00:00:00Z","agent":"w","id":"muft0005","kind":"note","text":"an old row with no by","v":2}"#,
    ];
    std::fs::write(log.join("x.jsonl"), lines.join("\n") + "\n").unwrap();
    let (ok, out, err) = fael(&d, &["import", "other"]);
    assert!(ok, "{err}");
    assert!(out.contains("imported 3 row(s)"), "{out}"); // 3 adds + 1 folded close
    assert!(out.contains("1 close(s) folded"), "{out}");
    assert!(err.contains("skipped (no `by`"), "{err}");
    let (_, out, _) = fael(&d, &["find", "--files", "src"]);
    assert!(out.contains("issue login loops → src/b.rs"), "{out}");
    assert!(!out.contains("picked x")); // closed by the folded close
    let (_, out, _) = fael(&d, &["import", "other"]);
    assert!(out.contains("imported 3 row(s)"), "{out}"); // twice is safe
}

#[test]
fn import_map_moves_a_subtree() {
    let d = repo();
    std::fs::create_dir_all(d.join("old-mem")).unwrap();
    std::fs::write(
        d.join("old-mem/log.jsonl"),
        "{\"v\":1,\"ts\":\"2026-01-01T00:00:00Z\",\"by\":\"w\",\"id\":\"m1\",\"kind\":\"note\",\"text\":\"t\",\"files\":[\"svc/a.rs\"]}\n",
    )
    .unwrap();
    let (ok, _, err) = fael(&d, &["import", "old-mem", "--map", "svc/=services/svc/"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["find", "--files", "services/svc/a.rs"]);
    assert!(out.contains("→ services/svc/a.rs"), "{out}");
    let (ok, _, err) = fael(&d, &["import", "old-mem", "--map", "no-equals-here"]);
    assert!(!ok && err.contains("--map"), "{err}");
}
