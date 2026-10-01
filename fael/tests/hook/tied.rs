//! Session start lists the open issues tied to this branch's work in full
//! (PLAN-fael-visible-secretary chunk 3), not only in the count line.

use super::{fael, git, json, repo};

/// PLAN-fael-visible-secretary chunk 3: an open issue tied to this branch's
/// work lists in full, not only in the count — filed on the start branch,
/// keyed by a key a branch row carries, or on the plan doc a branch row
/// anchors (`PLAN-demo.md` names `plan:demo`). An unrelated issue counts only.
#[test]
fn session_start_lists_issues_tied_to_the_branch() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("PLAN-demo.md"), "# demo\n").unwrap();
    let base = git(&d, &["branch", "--show-current"]);
    let add = |args: &[&str]| {
        let (ok, _, err) = fael(&d, args, "");
        assert!(ok, "{err}");
    };
    let issue = |text: &str, files: &str, key: &str| {
        add(&["add", "issue", text, "--files", files, "--key", key]);
    };
    issue("keyed elsewhere", "src/a.rs", "auth:session");
    issue("on the plan doc", "PLAN-demo.md", "demo:scope");
    issue("unrelated thing", "src/a.rs", "db:migrate");
    git(&d, &["checkout", "-q", "-b", "feat/x"]);
    add(&[
        "add",
        "note",
        "chunk handoff",
        "--files",
        "plan:demo",
        "--key",
        "plan:demo:handoff",
    ]);
    add(&[
        "add",
        "decision",
        "session keyed",
        "--files",
        "src/a.rs",
        "--key",
        "auth:session",
    ]);
    issue("filed on the branch", "src/a.rs", "ui:copy");
    let input = format!(r#"{{"cwd":{},"session_id":"s1"}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{out}");
    for tied in ["keyed elsewhere", "on the plan doc", "filed on the branch"] {
        assert!(out.contains(tied), "{tied}: {out}");
    }
    assert!(!out.contains("unrelated thing"), "{out}");
    assert!(
        out.contains("3 tied to this branch · 4 open issues — fael find --kind issue"),
        "{out}"
    );
    // a fresh branch has filed nothing, so nothing is tied: the count line alone
    git(&d, &["checkout", "-q", base.trim()]);
    git(&d, &["checkout", "-q", "-b", "feat/y"]);
    let input = format!(r#"{{"cwd":{},"session_id":"s2"}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{out}");
    assert!(!out.contains("keyed elsewhere"), "{out}");
    assert!(out.contains("fael: 4 open issues"), "{out}");
}
