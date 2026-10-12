//! PLAN-fael-parallel: an open PR (a review chunk, or done with `--pr N` and no `(#N)` on
//! `origin/HEAD` yet) still holds its scope, and `conflicts` says whether its branch merges.

use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
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

/// On a new branch from main: start the chunk, commit `file`, finish with `done`'s args.
fn ship(d: &Path, state: &Path, uid: &str, branch: &str, file: &str, done: &[&str]) {
    git(d, &["switch", "-q", "-c", branch, "main"]);
    fael(d, state, &["chunk", "start", uid]);
    std::fs::write(d.join(file), branch).unwrap();
    git(d, &["add", file]);
    git(d, &["commit", "-qm", branch]);
    fael(d, state, &[&["chunk", "done", uid, "h"], done].concat());
    git(d, &["switch", "-q", "main"]);
}

#[test]
fn an_open_pr_holds_its_scope_and_says_if_it_merges() {
    let tmp = std::env::temp_dir().join(format!("fael-board-prs-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("a.txt"), "base").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "init"]);
    let f = |args: &[&str]| fael(&d, &state, args);
    let add = |t: &str, scope: &str| f(&["chunk", "add", t, "--brief", "b", "--scope", scope]);
    let (open, gone, rev) = (
        add("open", "a.txt"),
        add("gone", "b.txt"),
        add("rev", "z.txt"),
    );
    ship(&d, &state, &open, "feat/open", "a.txt", &["--pr", "5"]);
    ship(&d, &state, &gone, "feat/gone", "b.txt", &["--pr", "6"]);
    ship(&d, &state, &rev, "feat/rev", "z.txt", &[]);
    // main moves on: a.txt changes under feat/open, and PR 6 is squash-merged
    std::fs::write(d.join("a.txt"), "main").unwrap();
    git(&d, &["commit", "-qam", "gone (#6)"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(
        &d,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    let (on_a, on_b) = (add("on a", "a.txt"), add("on b", "b.txt"));

    let v: Value = serde_json::from_str(&f(&["board", "--json"])).unwrap();
    let chunk = |u: &str| {
        let all = v["projects"][0]["chunks"].as_array().unwrap();
        all.iter().find(|c| c["uid"] == u).unwrap().clone()
    };
    assert_eq!(chunk(&open)["state"], "done");
    assert_eq!(chunk(&open)["conflicts"], serde_json::json!(["a.txt"]));
    assert_eq!(chunk(&on_a)["overlaps"], serde_json::json!([open]));
    assert_eq!(
        chunk(&rev)["conflicts"],
        serde_json::json!([]),
        "review merges clean"
    );
    assert!(
        chunk(&gone).get("conflicts").is_none(),
        "merged: not an open PR"
    );
    assert_eq!(
        chunk(&on_b)["overlaps"],
        serde_json::json!([]),
        "PR 6 is merged"
    );
    assert_eq!(
        chunk(&rev)["overlaps"],
        serde_json::json!([]),
        "z.txt: nobody else"
    );
}

/// Decision 01M4QJPX: `done --pr` on a branch that no longer merges names the files; done
/// still succeeds, and a clean branch prints no note.
#[test]
fn done_says_which_files_conflict_with_the_base() {
    let tmp = std::env::temp_dir().join(format!("fael-done-conflict-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("a.txt"), "base").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "init"]);
    let f = |args: &[&str]| fael(&d, &state, args);
    let (bad, ok) = (
        f(&["chunk", "add", "bad", "--brief", "b"]),
        f(&["chunk", "add", "ok", "--brief", "b"]),
    );
    // each branch starts before main moved
    let done = |uid: &str, branch: &str, file: &str| {
        git(&d, &["switch", "-q", "-c", branch, "main~1"]);
        f(&["chunk", "start", uid]);
        std::fs::write(d.join(file), branch).unwrap();
        git(&d, &["add", file]);
        git(&d, &["commit", "-qm", branch]);
        f(&["chunk", "done", uid, "h", "--pr", "5"])
    };
    // main moves on under a.txt; the checkout's fetched base is origin/main
    std::fs::write(d.join("a.txt"), "main").unwrap();
    git(&d, &["commit", "-qam", "main"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    let head = "refs/remotes/origin/HEAD";
    git(&d, &["symbolic-ref", head, "refs/remotes/origin/main"]);
    let out = done(&bad, "feat/bad", "a.txt");
    assert!(
        out.contains("note: feat/bad conflicts with origin/main in a.txt — rebase"),
        "{out}"
    );
    let out = done(&ok, "feat/ok", "b.txt");
    assert!(!out.contains("note:"), "{out}");
}
