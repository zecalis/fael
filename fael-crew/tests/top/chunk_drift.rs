//! A running chunk's drift: what the default branch did since its base — commits that touch its
//! files (not the ones its worktree already holds), rows closed since it started — in
//! `chunk status` (text and --json) and as `drift N` on its board row.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> String {
    let o = Command::new(crate::common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{args:?}: {err}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(d: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(d)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// A commit on `d` touching `file`, then pushed.
fn commit(d: &Path, file: &str, subject: &str) {
    std::fs::write(d.join(file), subject).unwrap();
    git(d, &["add", "-A"]);
    git(d, &["commit", "-qm", subject]);
    git(d, &["push", "-q", "origin", "main"]);
}

/// `(repo with a running chunk on src/x.rs, a second clone of origin to push main from, chunk uid)`
fn fixture() -> (PathBuf, PathBuf, String) {
    let tmp = std::env::temp_dir().join(format!("fael-drift-{}", fael_core::ulid()));
    let (bare, d, o) = (tmp.join("origin.git"), tmp.join("repo"), tmp.join("other"));
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    git(
        &tmp,
        &["clone", "-q", bare.to_str().unwrap(), d.to_str().unwrap()],
    );
    git(&d, &["switch", "-q", "-c", "main"]);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/x.rs"), "zero").unwrap();
    std::fs::write(d.join("other.txt"), "zero").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "init"]);
    git(&d, &["push", "-q", "-u", "origin", "main"]);
    git(&d, &["remote", "set-head", "origin", "main"]);
    git(
        &tmp,
        &["clone", "-q", bare.to_str().unwrap(), o.to_str().unwrap()],
    );
    fael(&d, &["plan", "new", "pl", "PL"]);
    let uid = fael(
        &d,
        &[
            "chunk", "add", "mover", "--plan", "pl", "--brief", "b", "--scope", "src/x.rs",
        ],
    );
    fael(&d, &["chunk", "start", &uid]);
    (d.canonicalize().unwrap(), o, uid)
}

fn running_line(d: &Path, uid: &str) -> String {
    let board = fael(d, &["board"]);
    board
        .lines()
        .find(|l| l.trim_start().starts_with("running") && l.contains(uid))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn main_moving_under_a_running_chunk_shows_in_status_and_on_the_board() {
    let (d, o, uid) = fixture();
    let ledger_id = fael(
        &d,
        &[
            "add",
            "issue",
            "x is wrong",
            "--title",
            "x",
            "--files",
            "src/x.rs",
        ],
    );
    let ledger_id = ledger_id.split_whitespace().next().unwrap().to_string();
    let quiet = fael(&d, &["chunk", "status", &uid]);
    assert!(quiet.contains("drift: main is 0 commit(s)"), "{quiet}");
    assert!(
        !running_line(&d, &uid).contains("drift"),
        "{}",
        running_line(&d, &uid)
    );

    // main: one commit on the chunk's file, one beside it; the agent takes both into its tree
    commit(&o, "src/x.rs", "feat: x one (#5)");
    commit(&o, "other.txt", "docs: other");
    git(&d, &["pull", "-q", "--ff-only"]);
    // then main moves again: x, other, and an issue closed that names x
    commit(&o, "src/x.rs", "feat: x two (#6)");
    commit(&o, "other.txt", "docs: other two");
    git(&d, &["fetch", "-q"]);
    fael(
        &d,
        &[
            "close",
            &ledger_id,
            "was wrong → fixed in x two; guard `tests/x/x_test.rs`",
        ],
    );

    let out = fael(&d, &["chunk", "status", &uid]);
    assert!(out.contains("drift: main is 4 commit(s)"), "{out}");
    assert!(out.contains("feat: x two (#6)"), "{out}");
    let block = out.split("drift:").nth(1).unwrap();
    for gone in ["x one", "docs: other"] {
        assert!(!block.contains(gone), "{gone} listed:\n{block}");
    }
    assert!(out.contains("fixed in x two"), "{out}");

    let v: serde_json::Value =
        serde_json::from_str(&fael(&d, &["chunk", "status", &uid, "--json"])).unwrap();
    let m = &v[0]["drift"];
    assert_eq!(m["behind"], 4, "{m}");
    assert_eq!(m["commits"].as_array().unwrap().len(), 1, "{m}");
    assert_eq!(m["closed"].as_array().unwrap().len(), 1, "{m}");

    assert!(
        running_line(&d, &uid).contains("drift 2"),
        "{}",
        running_line(&d, &uid)
    );
}
