//! `fael kickoff PLAN-x.md` (anchor `plan:x`, no file needed): at most 5 rows
//! without --limit, the plan's handoff first.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> String {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", root.join("state"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-kickplan-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
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

fn note(d: &Path, text: &str, key: &str) {
    fael(
        d,
        &[
            "add", "note", text, "--files", "plan:foo", "--key", key, "--force",
        ],
    );
}

#[test]
fn a_plan_kickoff_caps_at_five_rows() {
    let d = repo();
    for i in 0..8 {
        note(&d, &format!("other note {i}"), &format!("foo:topic-{i}"));
    }
    let rows = |out: &str| out.lines().filter(|l| l.starts_with("- [")).count();
    let out = fael(&d, &["kickoff", "PLAN-foo.md"]);
    assert_eq!(rows(&out), 5, "{out}");
    assert!(out.contains("--offset 5"), "{out}");
    // an explicit --limit wins, and a non-plan kickoff is not capped
    assert_eq!(
        rows(&fael(&d, &["kickoff", "PLAN-foo.md", "--limit", "7"])),
        7
    );
    assert!(rows(&fael(&d, &["kickoff"])) > 5);
}

fn rows(out: &str) -> Vec<&str> {
    out.lines().filter(|l| l.starts_with("- [")).collect()
}

#[test]
fn a_plan_path_from_a_subdirectory_names_the_same_plan() {
    let d = repo();
    std::fs::create_dir_all(d.join("docs/sub")).unwrap();
    note(&d, "chunk two note", "plan:foo:chunk-2");
    let out = fael(&d.join("docs/sub"), &["kickoff", "../PLAN-foo.md"]);
    assert!(out.contains("chunk two note"), "{out}");
}

#[test]
fn offset_pages_on_without_repeat_or_gap() {
    let d = repo();
    for i in 0..8 {
        note(&d, &format!("other note {i}"), &format!("foo:topic-{i}"));
    }
    let first = fael(&d, &["kickoff", "PLAN-foo.md"]);
    let second = fael(&d, &["kickoff", "PLAN-foo.md", "--offset", "5"]);
    let all = fael(&d, &["kickoff", "PLAN-foo.md", "--limit", "8"]);
    let mut paged: Vec<&str> = rows(&first);
    paged.extend(rows(&second));
    assert_eq!(paged, rows(&all), "{first}{second}");
}

#[test]
fn json_follows_the_same_cap() {
    let d = repo();
    for i in 0..8 {
        note(&d, &format!("other note {i}"), &format!("foo:topic-{i}"));
    }
    let out = fael(&d, &["kickoff", "PLAN-foo.md", "--json"]);
    // --json prints one row object per line
    let n = out.lines().filter(|l| l.starts_with('{')).count();
    assert_eq!(n, 5, "{out}");
}

#[test]
fn the_handoff_survives_the_cap_behind_many_open_issues() {
    let d = repo();
    note(&d, "the handoff note", "plan:foo:handoff");
    for i in 0..6 {
        fael(
            &d,
            &[
                "add",
                "issue",
                &format!("open issue {i}"),
                "--files",
                "plan:foo",
                "--key",
                &format!("foo:bug-{i}"),
                "--force",
            ],
        );
    }
    let out = fael(&d, &["kickoff", "PLAN-foo.md"]);
    assert!(out.contains("the handoff note"), "{out}");
}
