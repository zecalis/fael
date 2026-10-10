//! Journal-only rows carry their `@branch` tag into the hooks too — the edit
//! push and the session brief (PLAN-fael-durable-log chunk 1; 01M3HTE10).

use super::{fael, git, json, repo};

#[test]
fn hook_rows_carry_their_branch_tag() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "a"]);
    let main = git(&d, &["symbolic-ref", "--short", "HEAD"]);
    // a note filed on another branch, then that branch deleted — the journal
    // keeps it, and the edit push must tag it like `find` does (§5)
    git(&d, &["switch", "-qc", "feat/x"]);
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "row from another branch",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "rows"]);
    git(&d, &["switch", "-q", &main]);
    git(&d, &["branch", "-D", "feat/x"]);

    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "edit"], &input);
    assert!(ok && out.contains("row from another branch"), "{out}");
    assert!(out.contains("@feat/x"), "{out}");
}

/// The session brief tags a journal-only row too, not only the edit push
/// (01M3HTE10): an urgent issue from a deleted branch still lists with
/// `@branch`.
#[test]
fn session_brief_tags_a_journal_only_row() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "a"]);
    let main = git(&d, &["symbolic-ref", "--short", "HEAD"]);
    git(&d, &["switch", "-qc", "feat/z"]);
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "urgent from another branch",
            "--files",
            "src/a.rs",
            "--urgent",
        ],
        "",
    );
    assert!(ok, "{err}");
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "rows"]);
    git(&d, &["switch", "-q", &main]);
    git(&d, &["branch", "-D", "feat/z"]);

    let input = format!(r#"{{"cwd":{}}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok && out.contains("urgent from another branch"), "{out}");
    assert!(out.contains("@feat/z"), "{out}");
}
