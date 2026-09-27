//! `--help`, per-command help and the one-line errors: the binary in a
//! throwaway git repo.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"));
    let o = c.output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-help-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Test User"],
        &["config", "user.email", "t@example.com"],
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
    d
}

#[test]
fn per_command_help_and_short_errors() {
    let d = repo();
    // `fael <cmd> --help` shows only that command's section
    for args in [
        &["find", "--help"][..],
        &["find", "-h"][..],
        &["help", "find"][..],
    ] {
        let (ok, out, err) = fael(&d, args);
        assert!(ok, "{args:?} {err}");
        assert!(out.contains("fael find"), "{out}");
        assert!(!out.contains("fael mv"), "{out}");
    }
    // the full help lists every command plus global options and examples
    for args in [&["--help"][..], &["-h"][..], &["help"][..], &[][..]] {
        let (ok, out, err) = fael(&d, args);
        assert!(ok, "{args:?} {err}");
        assert!(
            out.contains("commands:")
                && out.contains("mv <old> <new>")
                && out.contains("global options:")
                && out.contains("examples:"),
            "{out}"
        );
    }
    // an unknown command or flag is one short line pointing at --help,
    // never a full usage dump on stderr
    let (ok, _, err) = fael(&d, &["frobnicate"]);
    assert!(!ok && err.contains("unknown command"), "{err}");
    assert!(!err.contains("commands:"), "{err}");
    let (ok, _, err) = fael(&d, &["find", "--nope"]);
    assert!(!ok && err.contains("unknown flag"), "{err}");
    assert!(!err.contains("commands:"), "{err}");
    // a real command with the wrong arity is not an "unknown command"
    for args in [&["close", "abc"][..], &["add"][..]] {
        let (ok, _, err) = fael(&d, args);
        assert!(!ok, "{args:?}");
        assert!(!err.contains("unknown command"), "{args:?} {err}");
        assert!(err.contains("wrong arguments"), "{args:?} {err}");
    }
    // after `--`, "-h" is the row's text, not a help request
    let (ok, out, err) = fael(&d, &["add", "note", "--files", "src/a.rs", "--", "-h"]);
    assert!(ok && !out.contains("fael add <kind>"), "{out} {err}");
    let (_, out, _) = fael(&d, &["find", "--full"]);
    assert!(out.contains("-h"), "{out}");
}
