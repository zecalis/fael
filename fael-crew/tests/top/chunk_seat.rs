//! `--seat` / `--cwd` (issue 01M6BK561): a call with no known directory answers from the
//! live run's seat; without `--seat` the fence still refuses a `done` from elsewhere.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
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
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
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

/// A repo with one commit and a second worktree `w2`; returns (main, w2).
fn two_worktrees() -> (PathBuf, PathBuf) {
    let d = std::env::temp_dir()
        .join(format!("fael-seat-{}", fael_core::ulid()))
        .canonicalize_or_self();
    let main = d.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let w2 = d.join("w2");
    git(
        &main,
        &["worktree", "add", "-q", "-b", "w2", w2.to_str().unwrap()],
    );
    (main, w2.canonicalize().unwrap())
}

trait CanonOrSelf {
    fn canonicalize_or_self(self) -> PathBuf;
}
impl CanonOrSelf for PathBuf {
    fn canonicalize_or_self(self) -> PathBuf {
        std::fs::create_dir_all(&self).unwrap();
        self.canonicalize().unwrap_or(self)
    }
}

#[test]
fn seat_finds_the_runs_worktree_and_cwd_names_one() {
    let (main, w2) = two_worktrees();
    let (ok, uid, err) = fael(&w2, &["chunk", "add", "Post", "--brief", "Write it"]);
    assert!(ok, "{err}");
    let uid = uid.trim();
    assert!(fael(&w2, &["chunk", "start", uid]).0);
    // from the other checkout, no --seat: the fence says where the run sits
    let (ok, _, err) = fael(
        &main,
        &["chunk", "done", uid, "h", "--left", "none", "--no-change"],
    );
    assert!(!ok && err.contains("was started in"), "{err}");
    // --seat: the run's own worktree answers, and the call says so
    let (ok, out, err) = fael(
        &main,
        &[
            "chunk",
            "done",
            uid,
            "h",
            "--left",
            "none",
            "--no-change",
            "--seat",
        ],
    );
    assert!(ok, "{err}");
    assert!(
        out.starts_with(&format!("seat: {} (run ", w2.display())),
        "{out}"
    );
    // nothing live now: --seat moves nowhere, the process dir answers as before
    let (_, out, err) = fael(
        &main,
        &["chunk", "wait", uid, "Q?", "--on", "owner", "--seat"],
    );
    assert!(
        !out.contains("seat:") && !err.contains("seat:"),
        "{out}{err}"
    );
    // --cwd is the CLI twin of the MCP cwd
    let (ok, out, err) = fael(
        &main,
        &["chunk", "show", uid, "--cwd", w2.to_str().unwrap()],
    );
    assert!(ok && out.contains(uid), "{out}{err}");
}

/// A uid this db lacks (another repo's) keeps the verb's own routing reject under `--seat`.
#[test]
fn seat_leaves_another_repos_chunk_to_the_routing_reject() {
    let (a, _) = two_worktrees();
    let (b, _) = two_worktrees();
    // one state dir, so repo a knows repo b exists
    let state = a.join("state");
    let run = |dir: &Path, args: &[&str]| {
        Command::new(crate::common::fael_bin())
            .args(args)
            .current_dir(dir)
            .env("FAEL_STATE_DIR", &state)
            .env_remove("FAEL_DIR")
            .output()
            .unwrap()
    };
    let o = run(&b, &["chunk", "add", "x", "--brief", "b"]);
    let uid = String::from_utf8_lossy(&o.stdout).trim().to_string();
    let args = [
        "chunk",
        "done",
        &uid,
        "h",
        "--left",
        "none",
        "--no-change",
        "--seat",
    ];
    let o = run(&a, &args);
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(!o.status.success() && err.contains("lives in"), "{err}");
}
