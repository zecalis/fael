//! Chunk 5 (PLAN-fael-row-hygiene): `add --revisit` stores the field,
//! `find --revisit` lists, kickoff wakes due dates from outside the file
//! filter and counts free text.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Per-child `FAEL_STATE_DIR` at `<repo root>/state`, so a real session on this
/// machine never leaks in and tests run in parallel without a global env lock.
fn state_env(c: &mut Command, dir: &Path) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    c.env("FAEL_STATE_DIR", root.join("state"));
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(dir);
    state_env(&mut c, dir);
    c.env_remove("CLAUDE_CODE_SESSION_ID");
    let o = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .unwrap()
        .wait_with_output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-revisit-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Revisit Test"],
        &["config", "user.email", "revisit@example.com"],
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
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    d
}

fn revisits(d: &Path) -> Vec<(String, String)> {
    revisits_with(d, true)
}

/// Live rows only (no `--all`) — what an agent's plain `find --revisit`
/// lists after a bump.
fn live_revisits(d: &Path) -> Vec<(String, String)> {
    revisits_with(d, false)
}

fn revisits_with(d: &Path, all: bool) -> Vec<(String, String)> {
    let mut args = vec!["find", "--json", "--revisit"];
    if all {
        args.push("--all");
    }
    let (_, out, _) = fael(d, &args);
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|v| {
            (
                v["text"].as_str().unwrap_or("").to_string(),
                v["revisit"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

#[test]
fn add_stores_revisit_and_find_lists_it() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "sleeping row",
            "--files",
            "src/a.rs",
            "--revisit",
            "2000-01",
        ],
    );
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["add", "note", "plain row", "--files", "src/b.rs"]);
    assert!(ok, "{err}");
    // bare --revisit lists only rows carrying the field …
    assert_eq!(revisits(&d), [("sleeping row".into(), "2000-01".into())]);
    // … and a value narrows to it
    let (ok, out, _) = fael(&d, &["find", "--revisit=2000"]);
    assert!(ok);
    assert!(out.contains("sleeping row"), "{out}");
    let (ok, out, _) = fael(&d, &["find", "--revisit=2999"]);
    assert!(ok);
    assert!(out.is_empty(), "{out}");
}

#[test]
fn add_rejects_bare_revisit() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "x", "--files", "src/a.rs", "--revisit"],
    );
    assert!(!ok, "bare --revisit on add must not be filed");
    assert!(err.contains("--revisit needs a value"), "{err}");
}

#[test]
fn bump_moves_revisit_without_supersede() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "note",
            "sleeper xyz",
            "--files",
            "src/a.rs",
            "--revisit",
            "2000-01",
        ],
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    // the new date replaces the old one on the live version …
    let (ok, _, err) = fael(&d, &["bump", &id, "--revisit", "2999-01"]);
    assert!(ok, "{err}");
    assert_eq!(
        live_revisits(&d),
        [("sleeper xyz".into(), "2999-01".into())]
    );
    // … so kickoff no longer wakes it, and the old date lists nothing
    let (ok, out, _) = fael(&d, &["kickoff", "src/b.rs"]);
    assert!(ok);
    assert!(!out.contains("sleeper xyz"), "{out}");
    let (ok, out, _) = fael(&d, &["find", "--revisit=2000"]);
    assert!(ok);
    assert!(out.is_empty(), "{out}");
    // no --revisit keeps the date on the next version
    let live = {
        let (_, out, _) = fael(&d, &["find", "--json", "--all", "--revisit"]);
        out.lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["text"].as_str() == Some("sleeper xyz"))
            .and_then(|v| v["id"].as_str().map(String::from))
            .unwrap()
    };
    let (ok, _, err) = fael(&d, &["bump", &live]);
    assert!(ok, "{err}");
    assert_eq!(
        live_revisits(&d),
        [("sleeper xyz".into(), "2999-01".into())]
    );
}

#[test]
fn bump_rejects_bare_revisit() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["add", "note", "x", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(&d, &["bump", &id, "--revisit"]);
    assert!(!ok, "bare --revisit on bump must not be filed");
    assert!(err.contains("--revisit needs a value"), "{err}");
}

#[test]
fn kickoff_wakes_due_and_counts_text() {
    let d = repo();
    for f in ["src/c.rs", "src/d.rs"] {
        std::fs::write(d.join(f), format!("// {f}\n")).unwrap();
    }
    // one file each: same-files repeats self-supersede now (chunk 3b), and
    // this fixture needs three open rows, not one replacing the rest
    for (text, revisit, f) in [
        ("due sleeper xyz", "2000-01", "src/a.rs"),
        ("future xyz", "2999-01", "src/c.rs"),
        ("text xyz", "mdl lands", "src/d.rs"),
    ] {
        let (ok, _, err) = fael(
            &d,
            &["add", "note", text, "--files", f, "--revisit", revisit],
        );
        assert!(ok, "{err}");
    }
    // scoped where nothing was filed: the due date still wakes up, the
    // future date and the free text stay out
    let (ok, out, _) = fael(&d, &["kickoff", "src/b.rs"]);
    assert!(ok);
    assert!(out.contains("due sleeper xyz"), "{out}");
    assert!(!out.contains("future xyz"), "{out}");
    assert!(!out.contains("text xyz"), "{out}");
    // the free text counts with a pointer to find
    assert!(out.contains("1 row waiting on revisit"), "{out}");
    assert!(out.contains("fael find --revisit"), "{out}");
}
