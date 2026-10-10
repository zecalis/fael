//! A session started in one worktree that edits a file in a sibling
//! worktree of the same clone (`git worktree add ../wt` then `cd ../wt`, as a
//! repo's "work in a worktree" rule asks) gets that file's rows: both share
//! the clone's journal, and a path is the same repo file in either.

use super::{fael, git, json, repo};

#[test]
fn a_sibling_worktree_file_gets_its_rows() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "a"]);
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let wt = d.with_file_name(format!("{}-wt", d.file_name().unwrap().to_string_lossy()));
    git(
        &d,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wt"],
    );
    let file = wt.join("src/a.rs");
    let input = format!(
        r#"{{"cwd":{},"session":"s1","files":[{}]}}"#,
        json(&d),
        json(&file)
    );
    let (ok, out, err) = fael(&d, &["hook", "edit"], &input);
    assert!(ok, "{err}");
    assert!(out.contains("login loops"), "{out}");
}

/// The shell way in (pilot 225-on-0): `cd` into the sibling worktree, then a
/// python edit that names the file by its relative path.
#[test]
fn a_shell_cd_into_a_sibling_worktree_edits_its_files() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "a"]);
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let wt = d.with_file_name(format!("{}-wt", d.file_name().unwrap().to_string_lossy()));
    git(
        &d,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wt"],
    );
    let cd = format!("cd {} && ", wt.join("src").display());
    std::fs::write(wt.join("src/a.rs"), "// b\n").unwrap(); // the edit's own write, just now
    let cmd = format!("{cd}python3 - <<'EOF'\np='a.rs'\nopen(p,'w').write('x')\nEOF");
    let input = serde_json::json!({"cwd": d, "session_id": "w", "tool_name": "Bash",
        "tool_input": {"command": cmd}, "tool_response": {"stdout": ""}});
    let (ok, out, err) = fael(
        &d,
        &["hook", "search", "--client", "claude"],
        &input.to_string(),
    );
    assert!(ok, "{err}");
    assert!(out.contains("login loops"), "{cmd}: {out}");
}

/// Another clone's file stays outside: its journal is not this one.
#[test]
fn another_clones_file_gets_nothing() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let other = repo();
    std::fs::write(other.join("src/a.rs"), "// a\n").unwrap();
    let input = format!(
        r#"{{"cwd":{},"session":"s1","files":[{}]}}"#,
        json(&d),
        json(&other.join("src/a.rs"))
    );
    let (ok, out, err) = fael(&d, &["hook", "edit"], &input);
    assert!(ok, "{err}");
    assert!(!out.contains("login loops"), "{out}");
}
