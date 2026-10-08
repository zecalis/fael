//! `fael kickoff PLAN-x.md`: at most 5 rows without --limit, and rows of a
//! chunk the plan ticked are left out (PLAN-fael-context-loop chunk 2).

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
    std::fs::write(
        d.join("PLAN-foo.md"),
        "- [x] chunk 1 — done\n- [ ] chunk 2 — open\n",
    )
    .unwrap();
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
fn a_plan_kickoff_drops_ticked_chunks_and_caps_at_five_rows() {
    let d = repo();
    note(&d, "chunk one note", "plan:foo:chunk-1");
    note(&d, "chunk two note", "plan:foo:chunk-2");
    for i in 0..6 {
        note(&d, &format!("other note {i}"), &format!("foo:topic-{i}"));
    }
    let rows = |out: &str| out.lines().filter(|l| l.starts_with("- [")).count();
    let out = fael(&d, &["kickoff", "PLAN-foo.md"]);
    assert!(!out.contains("chunk one note"), "{out}");
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
fn an_unreadable_plan_doc_still_caps_and_keeps_every_row() {
    let d = repo();
    note(&d, "chunk one note", "plan:foo:chunk-1");
    for i in 0..6 {
        note(&d, &format!("other note {i}"), &format!("foo:topic-{i}"));
    }
    std::fs::remove_file(d.join("PLAN-foo.md")).unwrap();
    // the doc is gone: the anchor still widens, nothing is filtered, the cap holds
    let out = fael(&d, &["kickoff", "PLAN-foo.md"]);
    assert_eq!(rows(&out).len(), 5, "{out}");
}

#[test]
fn a_nested_plan_doc_works_from_a_subdirectory() {
    let d = repo();
    std::fs::create_dir_all(d.join(".fapony/plan/sub")).unwrap();
    std::fs::rename(d.join("PLAN-foo.md"), d.join(".fapony/plan/PLAN-foo.md")).unwrap();
    note(&d, "chunk one note", "plan:foo:chunk-1");
    note(&d, "chunk two note", "plan:foo:chunk-2");
    let out = fael(&d, &["kickoff", ".fapony/plan/PLAN-foo.md"]);
    assert!(
        !out.contains("chunk one note") && out.contains("chunk two note"),
        "{out}"
    );
    let out = fael(&d.join(".fapony/plan/sub"), &["kickoff", "../PLAN-foo.md"]);
    assert!(
        !out.contains("chunk one note") && out.contains("chunk two note"),
        "{out}"
    );
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
