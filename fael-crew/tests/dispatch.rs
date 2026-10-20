//! `fael-crew` run directly and through `fael`: one help, the same unknown-flag reject, and the
//! hidden `fael held` that `chunk start` and `chunk done` call back into.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn crew(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael-crew"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_BIN", common::fael_bin())
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-dispatch-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    for a in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "Dispatch Test"],
        &["config", "user.email", "dispatch@example.com"],
    ] {
        Command::new("git")
            .args(a)
            .current_dir(&d)
            .status()
            .unwrap();
    }
    d
}

#[test]
fn the_crew_binary_and_fael_print_the_same_help_and_rejects() {
    let d = repo();
    for argv in [
        &["chunk", "--help"][..],
        &["plan", "--help"],
        &["chunk", "approve", "--help"],
        &["board", "--help"],
    ] {
        let (ok_c, out_c, _) = crew(&d, argv);
        let (ok_f, out_f, _) = fael(&d, argv);
        assert!(ok_c && ok_f, "{argv:?}");
        assert_eq!(out_c, out_f, "{argv:?}");
        assert!(out_c.starts_with("fael "), "{argv:?}: {out_c}");
    }
    // bare: the verbs it owns
    let (ok, out, _) = crew(&d, &["--help"]);
    assert!(
        ok && out.contains("chunk") && out.contains("plan") && out.contains("board"),
        "{out}"
    );
    // a reject is worded once, by either door
    let (ok_c, _, err_c) = crew(&d, &["board", "extra"]);
    let (ok_f, _, err_f) = fael(&d, &["board", "extra"]);
    assert!(!ok_c && !ok_f);
    assert_eq!(err_c, err_f);
    assert!(err_c.starts_with("rejected:"), "{err_c}");
}

#[test]
fn held_prints_nothing_for_a_repo_with_no_rows_and_rejects_an_unknown_kind() {
    let d = repo();
    let run = |args: &[&str]| {
        let o = Command::new(common::fael_bin())
            .args(args)
            .current_dir(&d)
            .env("FAEL_STATE_DIR", d.join("state"))
            .env_remove("FAEL_DIR")
            .output()
            .unwrap();
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    assert_eq!(run(&["held", "start"]), (true, String::new()));
    assert_eq!(run(&["held", "done"]), (true, String::new()));
    assert!(!run(&["held", "nope"]).0);
}
