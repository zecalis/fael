//! `fael chunk add|edit` warn on a path the stored brief names that exists on disk but is
//! outside the scope (plan fael-handover h3) — a warning only, the scope is never touched.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(crate::common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-brief-paths-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::create_dir_all(d.join("e2e")).unwrap();
    std::fs::write(d.join("src/x.rs"), "").unwrap();
    std::fs::write(d.join("e2e/y.ts"), "").unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    d
}

const BRIEF: &str = "edit src/x.rs and `e2e/y.ts:12`, then create src/new.rs";

fn add(d: &Path, plan: &str, brief: &str, scope: &str, label: &str) -> (String, String) {
    let args = [
        "chunk", "add", "T", "--plan", plan, "--brief", brief, "--scope", scope, "--label", label,
    ];
    let (ok, out, err) = fael(d, &args);
    assert!(ok, "{err}");
    (out.trim().to_string(), err)
}

#[test]
fn a_brief_path_outside_the_scope_warns_and_the_scope_is_unchanged() {
    let d = repo();
    let (uid, err) = add(&d, "inbox", BRIEF, "src/", "a1");
    assert!(
        err.contains("warning: brief names e2e/y.ts, not in scope"),
        "{err}"
    );
    assert!(
        err.contains(&format!("fael chunk edit {uid} --scope")),
        "{err}"
    );
    for quiet in ["src/x.rs", "src/new.rs"] {
        assert!(!err.contains(&format!("brief names {quiet}")), "{err}");
    }
    let (_, show, _) = fael(&d, &["chunk", "show", &uid, "--json"]);
    let v: serde_json::Value = serde_json::from_str(&show).unwrap();
    assert_eq!(v["scope"], serde_json::json!(["src/"]), "{show}");
}

#[test]
fn another_open_chunk_holding_the_path_is_named() {
    let d = repo();
    add(&d, "inbox", "do it", "e2e/", "holder");
    let (_, err) = add(&d, "inbox", BRIEF, "src/", "a1");
    assert!(
        err.contains("e2e/y.ts, not in scope (in inbox/holder's scope)"),
        "{err}"
    );
}

#[test]
fn edit_brief_rechecks_and_a_path_not_on_disk_is_ignored() {
    let d = repo();
    let (uid, err) = add(&d, "inbox", "edit src/x.rs and e2e/none.ts", "src/", "a1");
    assert!(!err.contains("warning"), "{err}");
    let (ok, _, err) = fael(&d, &["chunk", "edit", &uid, "--brief", "touch e2e/y.ts."]);
    assert!(
        ok && err.contains("brief names e2e/y.ts, not in scope"),
        "{err}"
    );
    let (ok, _, err) = fael(&d, &["chunk", "edit", &uid, "--title", "U"]);
    assert!(ok && !err.contains("warning"), "{err}");
}

#[test]
fn a_check_line_path_is_run_not_edit_so_it_never_warns() {
    let d = repo();
    let (_, err) = add(
        &d,
        "inbox",
        "Edit src/x.rs. Check: run e2e/y.ts suite, gate green.",
        "src/",
        "a1",
    );
    assert!(!err.contains("brief names"), "{err}");
    let (_, err) = add(
        &d,
        "inbox",
        "Edit e2e/y.ts. Check: gate green.",
        "src/",
        "a2",
    );
    assert!(
        err.contains("warning: brief names e2e/y.ts, not in scope"),
        "{err}"
    );
}
