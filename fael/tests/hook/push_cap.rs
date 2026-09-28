//! Read-push row cap (PLAN-fael-push-focus chunk 1): at most
//! `budget.push_rows` rows push, an open issue always shows, and one omitted
//! line names the exact `fael find --files …` next call.

use super::{fael, json, repo};
use std::path::Path;

fn seed(d: &Path) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    for i in 0..15 {
        let (ok, _, err) = fael(
            d,
            &[
                "add",
                "decision",
                &format!("decision {i}"),
                "--files",
                "src/a.rs",
            ],
            "",
        );
        assert!(ok, "{err}");
    }
}

fn read(d: &Path, session: Option<&str>) -> (bool, String) {
    let input = match session {
        Some(s) => format!(
            r#"{{"cwd":{},"session":"{s}","files":["src/a.rs"]}}"#,
            json(d)
        ),
        None => format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(d)),
    };
    let (ok, out, _) = fael(d, &["hook", "read"], &input);
    (ok, out)
}

fn shown(out: &str) -> usize {
    // the Reply is one JSON line — row lines are `\n- [` once unescaped
    out.matches("- [").count()
}

#[test]
fn read_push_caps_at_five_with_next_call() {
    let d = repo();
    seed(&d);
    let (ok, out) = read(&d, None);
    assert!(ok, "{out}");
    // 16 matching rows → 5 shown, the issue among them, plus one line
    // naming the exact next call (not the budget line)
    assert_eq!(shown(&out), 5, "{out}");
    assert!(out.contains("login loops"), "{out}");
    assert!(
        out.contains("… +11 more about this file — fael find --files src/a.rs"),
        "{out}"
    );
    assert!(!out.contains("narrow the filter"), "{out}");
}

#[test]
fn read_push_omitted_rows_push_later_in_session() {
    let d = repo();
    seed(&d);
    // omitted rows never reach seen, so later reads push them: 5 + 5 + 5 + 1
    let (ok, first) = read(&d, Some("cap1"));
    assert!(ok, "{first}");
    assert_eq!(shown(&first), 5, "{first}");
    assert!(first.contains("login loops"), "{first}");
    let (ok, second) = read(&d, Some("cap1"));
    assert!(ok, "{second}");
    assert_eq!(shown(&second), 5, "{second}");
    assert!(!second.contains("login loops"), "{second}");
    assert!(second.contains("… +6 more about this file — "), "{second}");
    let (ok, third) = read(&d, Some("cap1"));
    assert!(ok, "{third}");
    assert_eq!(shown(&third), 5, "{third}");
    let (ok, fourth) = read(&d, Some("cap1"));
    assert!(ok, "{fourth}");
    assert_eq!(shown(&fourth), 1, "{fourth}");
    assert!(!fourth.contains("more about this file"), "{fourth}");
}

#[test]
fn read_push_zero_rows_means_budget_only() {
    let d = repo();
    seed(&d);
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_rows = 0\n").unwrap();
    // no row cap: all 16 fit the default token budget, no omitted line
    let (ok, out) = read(&d, None);
    assert!(ok, "{out}");
    assert_eq!(shown(&out), 16, "{out}");
    assert!(!out.contains("more about this file"), "{out}");
}
