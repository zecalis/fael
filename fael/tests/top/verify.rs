//! `fael verify` through the real binary: recorded vs now, never an edit,
//! and a row another writer filed is never run.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Per-child `FAEL_STATE_DIR` at `<repo root>/state`, so a real session on
/// this machine never leaks in and tests run in parallel.
fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", root.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID");
    if !stdin.is_empty() {
        c.stdin(Stdio::piped());
    }
    let mut c = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if !stdin.is_empty() {
        c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    }
    let o = c.wait_with_output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-verify-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Verify Test"],
        &["config", "user.email", "verify@example.com"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&d)
                .status()
                .unwrap()
                .success()
        );
    }
    Command::new("git")
        .args(["commit", "-q", "--allow-empty", "-m", "init"])
        .current_dir(&d)
        .status()
        .unwrap();
    std::fs::write(d.join("src/a.rs"), "// a.rs\n").unwrap();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    d
}

fn note(d: &Path, text: &str) -> String {
    let (ok, out, err) = fael(d, &["add", "note", text, "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

#[test]
fn prints_recorded_beside_now_and_edits_nothing() {
    let d = repo();
    let same = note(&d, "hits\ncmd: echo 210\ndate: 2026-10-27\nresult: 210");
    let off = note(&d, "hits\ncmd: echo 210\ndate: 2026-10-27\nresult: 223");
    let log = || std::fs::read_to_string(d.join(".git/fael/log")).ok();
    let before = log();
    let (ok, out, _) = fael(&d, &["verify", &same], "");
    assert!(
        ok && out.contains("recorded: 210") && out.contains("now:      210"),
        "{out}"
    );
    let (ok, out, _) = fael(&d, &["verify", &off], "");
    assert!(
        !ok && out.contains("recorded: 223") && out.contains("now:      210"),
        "{out}"
    );
    assert!(out.trim_end().ends_with("differs"), "{out}");
    assert_eq!(before, log());
}

#[test]
fn rejects_a_row_without_the_lines_or_from_another_writer() {
    let d = repo();
    let plain = note(&d, "223 hits");
    let (ok, _, err) = fael(&d, &["verify", &plain], "");
    assert!(
        !ok && err.contains("rejected") && err.contains("cmd:"),
        "{err}"
    );
    let mine = note(&d, "hits\ncmd: touch ran\nresult: x");
    let git = |k, v| {
        Command::new("git")
            .args(["config", k, v])
            .current_dir(&d)
            .status()
            .unwrap();
    };
    git("user.email", "other@example.com");
    let (ok, _, err) = fael(&d, &["verify", &mine], "");
    assert!(!ok && err.contains("not this machine"), "{err}");
    assert!(
        !d.join("ran").exists(),
        "a foreign row's command must not run"
    );
}
