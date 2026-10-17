//! `project.branches[]` (decision 01M4V0FY): each local branch but the base's, `local` /
//! `pushed` / `merged` by git alone, and the unfinished chunks whose run branch it holds.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
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

fn git(dir: &Path, args: &[&str]) {
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    let ok = Command::new("git")
        .args(id)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A branch from main with one commit writing `file`; back on main after.
fn branch(d: &Path, name: &str, file: &str) {
    git(d, &["switch", "-q", "-c", name, "main"]);
    std::fs::write(d.join(file), name).unwrap();
    git(d, &["add", file]);
    git(d, &["commit", "-qm", name]);
    git(d, &["switch", "-q", "main"]);
}

#[test]
fn branches_say_push_state_and_the_chunks_a_stack_holds() {
    let tmp = std::env::temp_dir().join(format!("fael-board-branches-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("a.txt"), "base").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "init"]);
    // never fetched: its refs are written by hand below
    git(&d, &["remote", "add", "origin", "../nowhere"]);
    let lane = fael(&d, &state, &["chunk", "add", "lane", "--brief", "b"]);

    branch(&d, "talk", "t.txt");
    branch(&d, "pushed", "p.txt");
    git(&d, &["update-ref", "refs/remotes/origin/pushed", "pushed"]);
    git(&d, &["branch", "-q", "-u", "origin/pushed", "pushed"]);
    branch(&d, "gone", "g.txt");
    git(&d, &["update-ref", "refs/remotes/origin/gone", "gone"]);
    git(&d, &["branch", "-q", "-u", "origin/gone", "gone"]);
    git(&d, &["update-ref", "-d", "refs/remotes/origin/gone"]);
    // squash-merged: main carries its change as a commit of its own
    branch(&d, "squashed", "s.txt");
    std::fs::write(d.join("s.txt"), "squashed").unwrap();
    git(&d, &["add", "s.txt"]);
    git(&d, &["commit", "-qm", "squashed (#9)"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    let head = [
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ];
    git(&d, &head);
    // a stack cut from origin/main tracks it: still never pushed
    git(&d, &["branch", "-q", "-u", "origin/main", "talk"]);
    // a lane's run, landed on a stack no worktree holds
    git(&d, &["switch", "-q", "-c", "lane", "main"]);
    fael(&d, &state, &["chunk", "start", &lane]);
    std::fs::write(d.join("l.txt"), "lane").unwrap();
    git(&d, &["add", "l.txt"]);
    git(&d, &["commit", "-qm", "lane"]);
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["branch", "plan/p/stack", "lane"]);

    let v: Value = serde_json::from_str(&fael(&d, &state, &["board", "--json"])).unwrap();
    let got = &v["projects"][0]["branches"];
    let want = json!([
        {"name": "gone", "state": "merged", "chunks": []},
        {"name": "lane", "state": "local", "chunks": [lane]},
        {"name": "plan/p/stack", "state": "local", "chunks": [lane]},
        {"name": "pushed", "state": "pushed", "chunks": []},
        {"name": "squashed", "state": "merged", "chunks": []},
        {"name": "talk", "state": "local", "chunks": []},
    ]);
    assert_eq!(*got, want, "main is the base's own: never listed");
    let _ = std::fs::remove_dir_all(&tmp);
}
