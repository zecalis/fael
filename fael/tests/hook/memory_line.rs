//! The `memory: ~<used>/<budget> tokens · <n> rows` line on push and
//! session-start: an estimate that rides context already being injected,
//! shown only when rows were said.

use super::{fael, json, repo};
use std::path::Path;

fn add(d: &Path, args: &[&str]) {
    let mut a = vec!["add", "issue"];
    a.extend(args);
    let (ok, _, err) = fael(d, &a, "");
    assert!(ok, "{err}");
}

#[test]
fn push_says_what_the_rows_cost() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    add(&d, &["login loops", "--files", "src/a.rs"]);
    add(&d, &["token refresh races", "--files", "src/a.rs"]);
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "read"], &input);
    assert!(ok && out.contains("2 rows"), "{out}");
    // `~` marks an estimate; the budget is the configured push budget (default 800)
    assert!(
        out.contains("memory: ~") && out.contains("/800 tokens · 2 rows"),
        "{out}"
    );
}

#[test]
fn push_without_rows_has_no_line() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    add(&d, &["login loops", "--files", "src/a.rs"]);
    // b.rs has no rows (and is not in a.rs's directory tier on a read)
    let input = format!(r#"{{"cwd":{},"files":["src/b.rs"]}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "read"], &input);
    assert!(ok && !out.contains("memory:"), "{out}");
}

#[test]
fn session_start_says_it_only_when_rows_are_listed() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let input = format!(r#"{{"cwd":{}}}"#, json(&d));
    // a plain issue is counted, not listed — no rows, no line
    add(&d, &["anyone plain", "--files", "src/a.rs"]);
    let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok && !out.contains("memory:"), "{out}");
    // an urgent unowned issue lists in full — now the line appears
    add(&d, &["hot unowned", "--files", "src/a.rs", "--urgent"]);
    let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok && out.contains("hot unowned"), "{out}");
    assert!(
        out.contains("memory: ~") && out.contains("tokens · 1 row"),
        "{out}"
    );
}
