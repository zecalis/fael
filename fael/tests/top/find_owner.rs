//! `find` and `chunk show` misses that are not misses (issue 01M4XKM4): words passed as separate
//! args are one text query, and an id with no row here names where it lives —
//! a chunk in this repo, or a row in another repo on this machine.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-find-owner-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
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
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    d.canonicalize().unwrap()
}

/// `add` a note on its own file; returns the new row's id.
fn add(d: &Path, text: &str) -> String {
    std::fs::write(d.join("src/a.rs"), "// x\n").unwrap();
    let (ok, out, err) = fael(d, &["add", "note", text, "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

#[test]
fn words_as_separate_args_are_one_text_query() {
    let d = repo();
    let id = add(&d, "lead worker worktree layout");
    let (ok, out, err) = fael(&d, &["find", "lead", "worker", "worktree"]);
    assert!(ok, "{err}{out}");
    assert!(
        out.contains(&id[..8]) && !out.contains("not an id"),
        "{out}"
    );
}

#[test]
fn a_chunk_uid_shows_the_chunk() {
    let d = repo();
    let (ok, uid, err) = fael(&d, &["chunk", "add", "Post", "--brief", "Write it"]);
    assert!(ok, "{err}");
    let uid = uid.trim();
    let row = add(&d, "a row beside the chunk");
    // alone, and in a batch beside a row: the brief prints, no reject
    for args in [vec!["find", uid], vec!["find", uid, &row]] {
        let (ok, out, err) = fael(&d, &args);
        assert!(ok, "{out}{err}");
        assert!(
            out.contains(&format!("# Post ({uid})")) && out.contains("Write it"),
            "{out}"
        );
    }
    // --json stays machine-shaped: the chunk is a reject, not markdown
    let (ok, out, err) = fael(&d, &["find", uid, "--json"]);
    assert!(
        !ok && format!("{out}{err}").contains("is a chunk here"),
        "{out}{err}"
    );
}

#[test]
fn an_id_from_another_repo_names_that_repo() {
    let (here, there) = (repo(), repo());
    add(&here, "a row of this repo");
    let id = add(&there, "a row of the other repo");
    // usage names the other repo, as any fael call in it would; outside an agent
    // session no call wrote usage yet, so the state dir may not exist
    std::fs::create_dir_all(here.join("state")).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(here.join("state/usage.jsonl"))
        .unwrap();
    writeln!(f, "{}", serde_json::json!({"event": "call", "repo": there})).unwrap();
    let (ok, out, err) = fael(&here, &["find", &id]);
    assert!(!ok, "{out}");
    let all = format!("{out}{err}");
    let want = format!("{id} is a ledger note in {}", there.display());
    assert!(all.contains(&want), "{all}");
    // a token no repo owns stays a plain miss
    let (_, out, err) = fael(&here, &["find", "01M4ZZZZZZZZ"]);
    assert!(!format!("{out}{err}").contains(" is a "), "{out}{err}");
}

/// `chunk show` with another repo's uid names that repo (one lead tried four repos, 2026-10-14).
#[test]
fn chunk_show_names_the_repo_that_owns_the_uid() {
    let (here, there) = (repo(), repo());
    add(&here, "a row of this repo");
    let (ok, uid, err) = fael(&there, &["chunk", "add", "Post", "--brief", "Write it"]);
    assert!(ok, "{err}");
    let uid = uid.trim();
    std::fs::create_dir_all(here.join("state")).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(here.join("state/usage.jsonl"))
        .unwrap();
    writeln!(f, "{}", serde_json::json!({"event": "call", "repo": there})).unwrap();
    let (ok, _, err) = fael(&here, &["chunk", "show", uid]);
    let want = format!(
        "rejected: chunk {uid} lives in {} — cd {} && fael chunk show {uid}",
        there.display(),
        there.display(),
    );
    assert!(!ok && err.contains(&want), "{err}");
}
