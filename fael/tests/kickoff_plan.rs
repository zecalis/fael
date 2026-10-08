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
