//! The active plan (PLAN-fael-push-focus chunk 3): session-start prints one
//! `active plan:` line built from the newest plan-keyed row on the start
//! branch, pointing at the first `plan_dirs/PLAN-<name>.md` that exists —
//! and that row tops the push. No plan row, no line.

use super::{fael, git, json, repo};
use std::path::{Path, PathBuf};

/// A plan row on the read target `src/a.rs` filed on a fresh branch (the
/// session's start branch), and a fresher tier-0 decision on the same file
/// filed on the base branch — only the plan row is Now.
fn seed(d: &Path, plan_file: bool) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    if plan_file {
        std::fs::create_dir_all(d.join(".fapony/plan")).unwrap();
        std::fs::write(d.join(".fapony/plan/PLAN-foo.md"), "# foo\n").unwrap();
    }
    let (ok, _, err) = fael(
        d,
        &[
            "add",
            "decision",
            "fresher tier-0 decision",
            "--files",
            "src/a.rs",
            "--key",
            "db:migrate",
        ],
        "",
    );
    assert!(ok, "{err}");
    git(d, &["checkout", "-q", "-b", "feat/plan"]);
    let (ok, _, err) = fael(
        d,
        &[
            "add",
            "note",
            "chunk 3 handoff",
            "--files",
            "src/a.rs",
            "--key",
            "plan:foo:chunk-3",
        ],
        "",
    );
    assert!(ok, "{err}");
}

fn session_start(d: &Path, session: &str) -> String {
    let input = format!(r#"{{"cwd":{},"session":"{session}"}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "session-start"], &input);
    assert!(ok, "{err}");
    out
}

fn read(d: &Path, session: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":"{session}","files":["src/a.rs"]}}"#,
        json(d)
    );
    let (ok, out, err) = fael(d, &["hook", "read"], &input);
    assert!(ok, "{err}");
    out
}

fn focus_file(d: &Path) -> PathBuf {
    let mut out: Vec<PathBuf> = std::fs::read_dir(d.join("state/sessions"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    assert_eq!(out.len(), 1, "{out:?}");
    out.into_iter().next().unwrap()
}

#[test]
fn active_plan_line_points_at_the_plan_file() {
    let d = repo();
    seed(&d, true);
    let out = session_start(&d, "plan-1");
    assert!(
        out.contains("active plan: foo chunk-3 → .fapony/plan/PLAN-foo.md"),
        "{out}"
    );
    // the same Focus the push ranks with carries the resolved path
    let body = std::fs::read_to_string(focus_file(&d)).unwrap();
    assert!(
        body.contains(r#""plan":{"name":"foo","chunk":3,"path":".fapony/plan/PLAN-foo.md"}"#),
        "{body}"
    );
    // the plan row tops the push — ahead of the fresher tier-0 decision
    let out = read(&d, "plan-1");
    let plan_row = out
        .find("chunk 3 handoff")
        .unwrap_or_else(|| panic!("{out}"));
    let tier0 = out
        .find("fresher tier-0 decision")
        .unwrap_or_else(|| panic!("{out}"));
    assert!(plan_row < tier0, "plan row must lead: {out}");
    // a session with no Focus: L1's own order puts the decision first
    let out = read(&d, "plan-1-nofocus");
    let plan_row = out
        .find("chunk 3 handoff")
        .unwrap_or_else(|| panic!("{out}"));
    let tier0 = out
        .find("fresher tier-0 decision")
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        tier0 < plan_row,
        "freshness must decide with no Focus: {out}"
    );
}

#[test]
fn plan_row_without_a_file_says_the_chunk_anyway() {
    let d = repo();
    seed(&d, false);
    let out = session_start(&d, "plan-2");
    assert!(out.contains("active plan: foo chunk-3"), "{out}");
    assert!(
        !out.contains("active plan: foo chunk-3 →"),
        "no file, no arrow: {out}"
    );
}

#[test]
fn no_plan_row_no_line() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let out = session_start(&d, "plan-3");
    assert!(!out.contains("active plan:"), "{out}");
    assert!(out.contains("1 open issue"), "{out}");
    let body = std::fs::read_to_string(focus_file(&d)).unwrap();
    assert!(body.contains(r#""plan":null"#), "{body}");
}

#[test]
fn plan_dirs_config_decides_where_the_plan_lives() {
    let d = repo();
    seed(&d, false);
    std::fs::create_dir_all(d.join("docs/plans")).unwrap();
    std::fs::write(d.join("docs/plans/PLAN-foo.md"), "# foo\n").unwrap();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(
        d.join(".fael/config.toml"),
        "plan_dirs = [\"docs/plans\"]\n",
    )
    .unwrap();
    let out = session_start(&d, "plan-4");
    assert!(
        out.contains("active plan: foo chunk-3 → docs/plans/PLAN-foo.md"),
        "{out}"
    );
}
