//! The active plan (PLAN-fael-plan-focus chunk 1): session-start resolves it
//! from the open `plan:<name>:chunk-<n>` rows and the start branch — one plan
//! on the branch, or a single open plan anywhere, is `Active` and points at the
//! first `plan_dirs/PLAN-<name>.md` that exists; more than one is `?` and puts
//! no plan in Now. No plan row, no line.

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
    // the same Focus the push ranks with carries the resolution (no path —
    // session start resolves the path for the line only)
    let body = std::fs::read_to_string(focus_file(&d)).unwrap();
    assert!(
        body.contains(r#""plan":{"Active":{"name":"foo","chunk":3,"source":"Branch"}}"#),
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
fn plan_row_filed_on_an_earlier_branch_still_carries_the_plan() {
    // a chunk is worked on a fresh branch, so its plan rows sit on the
    // previous one (issue 01M3M35H) — with a single open plan, "only"
    // resolves it even though the session branch carries no plan row
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::create_dir_all(d.join(".fapony/plan")).unwrap();
    std::fs::write(d.join(".fapony/plan/PLAN-foo.md"), "# foo\n").unwrap();
    for (kind, text, key) in [
        ("decision", "fresher tier-0 decision", "db:migrate"),
        ("note", "chunk 3 handoff", "plan:foo:chunk-3"),
    ] {
        let (ok, _, err) = fael(
            &d,
            &["add", kind, text, "--files", "src/a.rs", "--key", key],
            "",
        );
        assert!(ok, "{err}");
    }
    // the session starts on a branch no plan row was filed on
    git(&d, &["checkout", "-q", "-b", "feat/chunk-4"]);
    let out = session_start(&d, "plan-5");
    assert!(
        out.contains("active plan: foo chunk-3 → .fapony/plan/PLAN-foo.md"),
        "{out}"
    );
    let out = read(&d, "plan-5");
    let plan_row = out
        .find("chunk 3 handoff")
        .unwrap_or_else(|| panic!("{out}"));
    let tier0 = out
        .find("fresher tier-0 decision")
        .unwrap_or_else(|| panic!("{out}"));
    assert!(plan_row < tier0, "the plan row tops the push: {out}");
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
    assert!(body.contains(r#""plan":"None""#), "{body}");
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

#[test]
fn two_open_plans_are_ambiguous_and_leave_now_empty() {
    // two open plans, neither filed on the session branch: the log cannot
    // choose, so the line says `?` and no plan row enters Now
    // (PLAN-fael-plan-focus chunk 1)
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    std::fs::create_dir_all(d.join(".fapony/plan")).unwrap();
    for name in ["alpha", "beta"] {
        std::fs::write(d.join(format!(".fapony/plan/PLAN-{name}.md")), "# plan\n").unwrap();
    }
    for (text, file, key) in [
        ("alpha handoff", "src/a.rs", "plan:alpha:chunk-1"),
        ("beta handoff", "src/b.rs", "plan:beta:chunk-1"),
    ] {
        let (ok, _, err) = fael(
            &d,
            &["add", "note", text, "--files", file, "--key", key],
            "",
        );
        assert!(ok, "{err}");
    }
    // a fresher tier-0 decision on src/a.rs — only it can lead while the plan
    // is ambiguous; a Now plan row would jump ahead of it
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "fresher decision",
            "--files",
            "src/a.rs",
            "--key",
            "db:migrate",
        ],
        "",
    );
    assert!(ok, "{err}");
    // a fresh branch carries neither plan row
    git(&d, &["checkout", "-q", "-b", "feat/fresh"]);
    let out = session_start(&d, "two-plans");
    assert!(out.contains("active plan: ? — 2 open:"), "{out}");
    assert!(out.contains("alpha chunk-1"), "{out}");
    assert!(out.contains("beta chunk-1"), "{out}");
    // no setter exists yet, so the line must not advertise one
    assert!(!out.contains("fael focus"), "{out}");
    // neither plan row is Now: the fresher decision leads, the alpha note
    // does not jump ahead through the plan key
    let out = read(&d, "two-plans");
    let dec = out
        .find("fresher decision")
        .unwrap_or_else(|| panic!("{out}"));
    let alpha = out.find("alpha handoff").unwrap_or_else(|| panic!("{out}"));
    assert!(dec < alpha, "ambiguous plan must not lead the push: {out}");
}
