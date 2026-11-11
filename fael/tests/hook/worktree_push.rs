//! A session started in one worktree that edits a file in a sibling
//! worktree of the same clone (`git worktree add ../wt` then `cd ../wt`, as a
//! repo's "work in a worktree" rule asks) gets that file's rows: both share
//! the clone's journal, and a path is the same repo file in either.

use super::{fael, fael_at, git, json, repo};

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

/// Same file, no chunk: another session in a sibling worktree edited
/// `src/a.rs` two minutes ago → one line naming its client, branch and age;
/// the next edit stays silent; eleven minutes apart stays silent.
#[test]
fn peer_same_file_across_worktrees_says_once() {
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
    let branch = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let wt = d.with_file_name(format!("{}-wt", d.file_name().unwrap().to_string_lossy()));
    git(
        &d,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wt"],
    );
    let state = d.join("state");
    let shift_all = |mins: i64| {
        let dir = state.join("sessions").join("edits");
        let now = fael_core::now_ms() as i64 - mins * 60_000;
        let at = fael_core::rfc3339(now as u64);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "jsonl") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let mut out = String::new();
            for line in text.lines() {
                let Ok(mut v) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                v["at"] = at.clone().into();
                out.push_str(&serde_json::to_string(&v).unwrap());
                out.push('\n');
            }
            let _ = std::fs::write(&p, out);
        }
    };
    // session A edits in the main worktree
    let input_a = format!(
        r#"{{"cwd":{},"client":"claude","session":"sess-a","files":[{}]}}"#,
        json(&d),
        json(&d.join("src/a.rs"))
    );
    let (ok, _, err) = fael_at(&state, &d, &["hook", "edit"], &input_a);
    assert!(ok, "{err}");
    shift_all(2);
    // session B edits the same repo-relative file in the sibling worktree
    let input_b = format!(
        r#"{{"cwd":{},"client":"claude","session":"sess-b","files":[{}]}}"#,
        json(&wt),
        json(&wt.join("src/a.rs"))
    );
    let (ok, out, err) = fael_at(&state, &wt, &["hook", "edit"], &input_b);
    assert!(ok, "{err}");
    assert!(out.contains("also edited by claude"), "{out}");
    assert!(out.contains(&branch), "{out}");
    assert!(out.contains("2m ago"), "{out}");
    assert!(out.contains("fael add note"), "{out}");
    assert!(out.contains("--files src/a.rs"), "{out}");
    // B's next edit of the file is silent (said once per file per session)
    let (ok, out, err) = fael_at(&state, &wt, &["hook", "edit"], &input_b);
    assert!(ok, "{err}");
    assert!(!out.contains("also edited"), "{out}");
    // eleven minutes apart is silent (outside the 10 min window)
    shift_all(11);
    let input_c = format!(
        r#"{{"cwd":{},"client":"claude","session":"sess-c","files":[{}]}}"#,
        json(&wt),
        json(&wt.join("src/a.rs"))
    );
    let (ok, out, err) = fael_at(&state, &wt, &["hook", "edit"], &input_c);
    assert!(ok, "{err}");
    assert!(!out.contains("also edited"), "{out}");
}
