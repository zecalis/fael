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
    // the default help lists the 7 core commands plus the unguided GAP
    // (next/keys/compact/intake stay shown — owner 2026-10-19) and the --all
    // line; chunk/plan/board stay hidden until --all
    for args in [&["--help"][..], &["-h"][..], &["help"][..], &[][..]] {
        let (ok, out, err) = fael(&d, args);
        assert!(ok, "{args:?} {err}");
        assert!(
            out.contains("commands:")
                && out.contains("global options:")
                && out.contains("examples:")
                && out.contains("fael --help --all"),
            "{out}"
        );
        for present in [
            "add", "find", "close", "bump", "claim", "doctor", "stats", "next", "keys", "compact",
            "intake",
        ] {
            assert!(
                out.contains(&format!("  {present}")),
                "{present} missing: {out}"
            );
        }
        assert!(
            !out.contains("more commands:"),
            "core help lists the rest: {out}"
        );
        for hidden in ["chunk", "plan", "board", "kickoff", "install", "mv"] {
            assert!(
                !out.lines().any(|l| l.trim_start().starts_with(hidden)),
                "{hidden} leaks into core help: {out}"
            );
        }
    }
    // `--help --all` is the full list again
    for args in [&["--help", "--all"][..], &["help", "--all"][..]] {
        let (ok, out, err) = fael(&d, args);
        assert!(ok, "{args:?} {err}");
        assert!(
            out.contains("commands:")
                && out.contains("more commands:")
                && out.contains("mv <old> <new>"),
            "{out}"
        );
        for present in ["chunk", "plan", "board"] {
            assert!(
                out.contains(&format!("  {present}")),
                "{present} missing: {out}"
            );
        }
    }
    // an unknown command or flag is one short line pointing at --help,
    // never a full usage dump on stderr
    let (ok, _, err) = fael(&d, &["frobnicate"]);
    assert!(!ok && err.contains("unknown command"), "{err}");
    // far from every name: the closed list, in the same one line
    assert!(err.contains("commands: add, close"), "{err}");
    assert_eq!(err.trim().lines().count(), 1, "{err}");
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
    // `--files a b` (a space, not a comma) files the row with both paths
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "spread", "--files", "src/a.rs", "src/b.rs"],
    );
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["find", "spread", "--json"]);
    assert!(
        out.contains("src/a.rs") && out.contains("src/b.rs"),
        "{out}"
    );
    // after `--`, "-h" is the row's text, not a help request
    let (ok, out, err) = fael(&d, &["add", "note", "--files", "src/a.rs", "--", "-h"]);
    assert!(ok && !out.contains("fael add <kind>"), "{out} {err}");
    let (_, out, _) = fael(&d, &["find", "--full"]);
    assert!(out.contains("-h"), "{out}");
}

#[test]
fn core_help_hides_advanced_flags_and_all_shows_them() {
    let d = repo();
    // `fael find --help` is the core: everyday flags only (tier C), no workflow ones
    let (ok, out, err) = fael(&d, &["find", "--help"]);
    assert!(ok, "{err}");
    // the footer names `--help --all` as the way to the hidden flags
    assert!(
        out.contains("run 'fael find --help --all' for its hidden flags."),
        "{out}"
    );
    let out = out.split("\n\nrun 'fael --help'").next().unwrap();
    for present in ["--files", "--key", "--limit", "--offset"] {
        assert!(out.contains(present), "{present} missing: {out}");
    }
    for hidden in [
        "--since",
        "--by",
        "--to",
        "--revisit",
        "--text",
        "--recall",
        "--kind",
        "--full",
        "--all",
    ] {
        assert!(!out.contains(hidden), "{hidden} leaks: {out}");
    }
    assert!(!out.contains("--recall"), "recall leaks: {out}");
    // `--help --all` is the full surface again
    let (ok, full, err) = fael(&d, &["find", "--help", "--all"]);
    assert!(ok, "{err}");
    for hidden in ["--since", "--recall", "--kind", "--full", "--all"] {
        assert!(full.contains(hidden), "{hidden} missing: {full}");
    }
    // same for add: workflow flags hide (--urgent/--revisit/--force/--dry-run), core stays
    let (ok, out, err) = fael(&d, &["add", "--help"]);
    assert!(ok, "{err}");
    for present in [
        "--title",
        "--files",
        "--key",
        "--to",
        "--from",
        "--supersedes",
        "--json",
    ] {
        assert!(out.contains(present), "{present} missing: {out}");
    }
    for hidden in [
        "--urgent",
        "--urgent-before",
        "--revisit",
        "--force",
        "--dry-run",
    ] {
        assert!(!out.contains(hidden), "{hidden} leaks: {out}");
    }
    let (ok, full, err) = fael(&d, &["add", "--help", "--all"]);
    assert!(ok, "{err}");
    for hidden in ["--force", "--dry-run", "--urgent-before", "--revisit"] {
        assert!(full.contains(hidden), "{hidden} missing: {full}");
    }
    // bump: every flag is workflow, so the core is the bare id
    let (ok, out, err) = fael(&d, &["bump", "--help"]);
    assert!(ok, "{err}");
    assert!(out.contains("fael bump <id>"), "{out}");
    for hidden in ["--to", "--revisit", "--urgent", "--not-urgent", "--why"] {
        assert!(!out.contains(hidden), "{hidden} leaks: {out}");
    }
    let (ok, full, err) = fael(&d, &["bump", "--help", "--all"]);
    assert!(ok, "{err}");
    assert!(full.contains("--to"), "{full}");
    // doctor: --fat hides, --fix/--json stay
    let (ok, out, err) = fael(&d, &["doctor", "--help"]);
    assert!(ok, "{err}");
    assert!(out.contains("--fix"), "{out}");
    assert!(!out.contains("--fat"), "--fat leaks: {out}");
    let (ok, full, err) = fael(&d, &["doctor", "--help", "--all"]);
    assert!(ok, "{err}");
    assert!(full.contains("--fat"), "{full}");
    // stats: --rows/--day/--misses hide, --json/--since stay
    let (ok, out, err) = fael(&d, &["stats", "--help"]);
    assert!(ok, "{err}");
    for present in ["--json", "--since"] {
        assert!(out.contains(present), "{present} missing: {out}");
    }
    for hidden in ["--rows", "--day", "--misses"] {
        assert!(!out.contains(hidden), "{hidden} leaks: {out}");
    }
    let (ok, full, err) = fael(&d, &["stats", "--help", "--all"]);
    assert!(ok, "{err}");
    for hidden in ["--rows", "--day", "--misses"] {
        assert!(full.contains(hidden), "{hidden} missing: {full}");
    }
    // no core override: close/claim show the same text either way;
    // hidden workflow commands keep their full own --help
    for cmd in ["close", "claim", "chunk", "plan", "board"] {
        let (_, core, _) = fael(&d, &[cmd, "--help"]);
        let (_, full, _) = fael(&d, &[cmd, "--help", "--all"]);
        assert_eq!(core, full, "{cmd}");
    }
}

#[test]
fn hidden_flags_still_run() {
    let d = repo();
    // every hidden CLI flag still parses and runs — hiding is discovery only
    for args in [
        &["find", "--since", "2026-01"][..],
        &["find", "--by", "nobody"],
        &["find", "--to", "nobody"],
        &["find", "--revisit"],
        &["find", "--recall", "wide"][..],
        &["find", "--kind", "issue"],
        &["find", "--full"],
        &["find", "--all"],
        &["find", "--text", "nothing-here"],
        &["find", "--limit", "1"],
    ] {
        let (ok, _, err) = fael(&d, args);
        assert!(ok, "{args:?} {err}");
    }
    // hidden bump/doctor/stats flags still run too
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(&d, &["add", "note", "probe-bump", "--files", "src/a.rs"]);
    assert!(ok, "{err} {out}");
    // hidden add flags still parse (revisit/urgent are workflow-hidden, not removed)
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "probe-hidden-flags",
            "--title",
            "probe hidden",
            "--files",
            "src/a.rs",
            "--revisit",
            "tomorrow",
            "--urgent",
        ],
    );
    assert!(ok, "{err}");
    // bump --to still parses: unknown id fails on the id, never on the flag
    let (ok, _, err) = fael(
        &d,
        &["bump", "01J00000000000000000000000", "--to", "nobody"],
    );
    assert!(!ok, "bump with a bad id should fail");
    assert!(
        !err.contains("unknown flag"),
        "bump --to no longer parses: {err}"
    );
    let (ok, _, err) = fael(&d, &["doctor", "--fat"]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["stats", "--rows"]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["stats", "--misses"]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["stats", "--day"]);
    assert!(ok, "{err}");
    // --dry-run writes nothing but still previews
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &["add", "note", "probe", "--files", "src/a.rs", "--dry-run"],
    );
    assert!(ok, "{err}");
    assert!(!out.is_empty(), "dry-run prints the verdict");
    // did-you-mean still knows the hidden flags (reads the full surface)
    let (ok, _, err) = fael(&d, &["find", "--groups"]);
    assert!(!ok && err.contains("fael find --kind issue"), "{err}");
    let (ok, _, err) = fael(&d, &["find", "--stale"]);
    assert!(!ok && err.contains("fael find --kind issue"), "{err}");
}
