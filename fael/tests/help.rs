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

#[test]
fn core_help_hides_advanced_flags_and_all_shows_them() {
    let d = repo();
    // `fael find --help` is the core: the everyday flags, none hidden ones
    let (ok, out, err) = fael(&d, &["find", "--help"]);
    assert!(ok, "{err}");
    for present in [
        "--files", "--key", "--kind", "--full", "--all", "--limit", "--offset",
    ] {
        assert!(out.contains(present), "{present} missing: {out}");
    }
    for hidden in [
        "--since",
        "--by",
        "--to",
        "--revisit",
        "--branches",
        "--groups",
    ] {
        assert!(!out.contains(hidden), "{hidden} leaks: {out}");
    }
    // `--help --all` is the full surface again
    let (ok, full, err) = fael(&d, &["find", "--help", "--all"]);
    assert!(ok, "{err}");
    for hidden in ["--since", "--branches", "--groups"] {
        assert!(full.contains(hidden), "{hidden} missing: {full}");
    }
    // same for add: --force/--dry-run/--urgent-before hide, --urgent stays
    let (ok, out, err) = fael(&d, &["add", "--help"]);
    assert!(ok, "{err}");
    assert!(out.contains("--urgent"), "{out}");
    for hidden in ["--force", "--dry-run", "--urgent-before"] {
        assert!(!out.contains(hidden), "{hidden} leaks: {out}");
    }
    let (ok, full, err) = fael(&d, &["add", "--help", "--all"]);
    assert!(ok, "{err}");
    for hidden in ["--force", "--dry-run", "--urgent-before"] {
        assert!(full.contains(hidden), "{hidden} missing: {full}");
    }
    // no core override: close shows the same text either way
    let (_, core, _) = fael(&d, &["close", "--help"]);
    let (_, full, _) = fael(&d, &["close", "--help", "--all"]);
    assert_eq!(core, full);
}

#[test]
fn hidden_flags_still_run() {
    let d = repo();
    // every hidden CLI flag still parses and runs — hiding is discovery only
    for args in [
        &["find", "--groups"][..],
        &["find", "--since", "2026-01"],
        &["find", "--by", "nobody"],
        &["find", "--to", "nobody"],
        &["find", "--revisit"],
        &["find", "--branches"],
    ] {
        let (ok, _, err) = fael(&d, args);
        assert!(ok, "{args:?} {err}");
    }
    // --dry-run writes nothing but still previews
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &["add", "note", "probe", "--files", "src/a.rs", "--dry-run"],
    );
    assert!(ok, "{err}");
    assert!(!out.is_empty(), "dry-run prints the verdict");
    // did-you-mean still knows the hidden flags (reads the full surface)
    let (ok, _, err) = fael(&d, &["find", "--group"]);
    assert!(!ok && err.contains("did you mean --groups?"), "{err}");
    let (ok, _, err) = fael(&d, &["find", "--stale"]);
    assert!(!ok && err.contains("fael find --kind issue"), "{err}");
}
