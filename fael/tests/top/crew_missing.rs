//! `fael` without `fael-crew` beside it or on PATH: the crew verbs say how to install it and
//! exit 1; the core verbs (add, find, close) never notice. The binary is copied to a bare
//! directory, so no sibling `fael-crew` is found, and PATH holds only git. Unix: the PATH is a symlink.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn bare_fael() -> (PathBuf, PathBuf) {
    let d = std::env::temp_dir().join(format!("fael-nocrew-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("bin")).unwrap();
    let bin = d
        .join("bin")
        .join(format!("fael{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_fael"), &bin).unwrap();
    let work = d.join("work");
    std::fs::create_dir_all(&work).unwrap();
    for a in [
        &["init", "-q"][..],
        &["config", "user.name", "Nocrew Test"],
        &["config", "user.email", "nocrew@example.com"],
    ] {
        Command::new("git")
            .args(a)
            .current_dir(&work)
            .status()
            .unwrap();
    }
    (bin, work)
}

/// PATH with `git` only: a symlink to the real git in an otherwise empty directory.
fn git_only_path(into: &Path) -> std::ffi::OsString {
    let dir = into.join("path");
    std::fs::create_dir_all(&dir).unwrap();
    let git = String::from_utf8(Command::new("which").arg("git").output().unwrap().stdout).unwrap();
    let _ = std::os::unix::fs::symlink(git.trim(), dir.join("git"));
    dir.into()
}

fn run(bin: &Path, work: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(bin)
        .args(args)
        .current_dir(work)
        .env("PATH", git_only_path(work.parent().unwrap()))
        .env("FAEL_STATE_DIR", work.parent().unwrap().join("state"))
        .env_remove("FAEL_BIN")
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

#[test]
fn crew_verbs_name_the_install_command_and_core_verbs_still_work() {
    let (bin, work) = bare_fael();
    for argv in [
        &["plan", "export", "x"][..],
        &["chunk", "show", "x"],
        &["board", "--json"],
    ] {
        let (ok, out, err) = run(&bin, &work, argv);
        assert!(!ok, "{argv:?}: {out}");
        assert!(
            err.contains("cargo install --path fael-crew"),
            "{argv:?}: {err}"
        );
    }
    let (ok, out, err) = run(
        &bin,
        &work,
        &["add", "note", "n1", "--title", "n1", "--files", "x.rs"],
    );
    assert!(ok, "{out}{err}");
    let (ok, out, err) = run(&bin, &work, &["find", "--files", "x.rs"]);
    assert!(ok && out.contains("n1"), "{out}{err}");
    let id = out.split(['[', ']']).nth(1).unwrap_or_default().to_string();
    let (ok, out, err) = run(&bin, &work, &["close", &id, "done"]);
    assert!(ok, "{out}{err}");
    // help for the crew verbs still prints without it
    let (ok, out, _) = run(&bin, &work, &["chunk", "--help"]);
    assert!(ok && out.contains("fael chunk start <uid>"), "{out}");
    // help for the core still prints without crew
    let (ok, out, _) = run(&bin, &work, &["--help"]);
    assert!(ok && out.contains("fael add"), "{out}");
}
