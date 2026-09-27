//! `import` through the real binary: the fapony legacy no-drop rule and
//! `--map` subtree moves.

use super::{fael, repo};

#[test]
fn import_fapony_legacy_drops_nothing() {
    let d = repo();
    let mem = d.join(".fapony").join(".memory");
    std::fs::create_dir_all(&mem).unwrap();
    let lines = [
        r#"{"ts":"2026-01-01T00:00:00Z","agent":"delamind","id":"muft0001","kind":"decision","text":"picked x","files":["src/a.rs"],"v":2}"#,
        r#"{"ts":"2026-01-02T00:00:00Z","agent":"delamind","id":"muft0002","kind":"bug","text":"login loops","files":["src/b.rs"],"v":2}"#,
        r#"{"ts":"2026-01-03T00:00:00Z","agent":"delamind","kind":"note","text":"no id","files":["src/c.rs"],"v":2}"#,
        r#"{"ts":"2026-01-04T00:00:00Z","agent":"delamind","id":"muft0004","kind":"close","ref":"muft0001","text":"done","files":[],"v":2}"#,
    ];
    std::fs::write(mem.join("log.delamind.jsonl"), lines.join("\n") + "\n").unwrap();
    let (ok, out, err) = fael(&d, &["import", ".fapony/.memory"]);
    assert!(ok, "{err}");
    assert!(out.contains("imported 3 row(s)"), "{out}"); // 4 lines = 3 adds + 1 folded close
    assert!(out.contains("1 close(s) folded"), "{out}");
    let (_, out, _) = fael(&d, &["find", "--files", "src"]);
    assert!(out.contains("issue login loops → src/b.rs"), "{out}");
    assert!(!out.contains("picked x")); // closed by the folded close
    let (_, out, _) = fael(&d, &["import", ".fapony/.memory"]);
    assert!(out.contains("imported 3 row(s)"), "{out}"); // twice is safe
    let (_, out, _) = fael(&d, &["keys"]);
    assert!(out.is_empty() || !out.contains("×0"), "{out}");
}

#[test]
fn import_map_moves_a_subtree() {
    let d = repo();
    std::fs::create_dir_all(d.join("old-mem")).unwrap();
    std::fs::write(
        d.join("old-mem/log.jsonl"),
        "{\"ts\":\"2026-01-01T00:00:00Z\",\"agent\":\"w\",\"id\":\"m1\",\"kind\":\"note\",\"text\":\"t\",\"files\":[\"svc/a.rs\"],\"v\":2}\n",
    )
    .unwrap();
    let (ok, _, err) = fael(&d, &["import", "old-mem", "--map", "svc/=services/svc/"]);
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["find", "--files", "services/svc/a.rs"]);
    assert!(out.contains("→ services/svc/a.rs"), "{out}");
    let (ok, _, err) = fael(&d, &["import", "old-mem", "--map", "no-equals-here"]);
    assert!(!ok && err.contains("--map"), "{err}");
}
