//! A review chunk whose run branch git says is on the base reads `merged: true` on the
//! board (no state change on read, decision 01M4V3A0D); `chunk accept --merged` lands
//! exactly those, and the rest stay in review.

use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(crate::common::fael_bin())
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

fn ship(d: &Path, state: &Path, uid: &str, branch: &str, file: &str) {
    git(d, &["switch", "-q", "-c", branch, "main"]);
    fael(d, state, &["chunk", "start", uid]);
    std::fs::write(d.join(file), branch).unwrap();
    git(d, &["add", file]);
    git(d, &["commit", "-qm", branch]);
    fael(d, state, &["chunk", "done", uid, "h", "--left", "none"]);
    git(d, &["switch", "-q", "main"]);
}

#[test]
fn accept_merged_lands_only_the_review_chunks_on_the_base() {
    let tmp = std::env::temp_dir().join(format!("fael-board-merged-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("a.txt"), "base").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "init"]);
    let f = |args: &[&str]| fael(&d, &state, args);
    let add = |t: &str, scope: &str| f(&["chunk", "add", t, "--brief", "b", "--scope", scope]);
    let (landed, waiting) = (add("landed", "x.txt"), add("waiting", "y.txt"));
    let closed = add("closed", "z.txt");
    ship(&d, &state, &landed, "feat/landed", "x.txt");
    ship(&d, &state, &waiting, "feat/waiting", "y.txt");
    ship(&d, &state, &closed, "feat/closed", "z.txt");
    // a PR closed unmerged: its upstream is gone, its content is not on the base
    git(&d, &["remote", "add", "origin", "/nonexistent"]);
    git(&d, &["config", "branch.feat/closed.remote", "origin"]);
    git(
        &d,
        &[
            "config",
            "branch.feat/closed.merge",
            "refs/heads/feat/closed",
        ],
    );
    // the owner squash-merges feat/landed
    git(&d, &["merge", "-q", "--squash", "feat/landed"]);
    git(&d, &["commit", "-qm", "landed (#1)"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(
        &d,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );

    let board = || -> Value { serde_json::from_str(&f(&["board", "--json"])).unwrap() };
    let chunk = |v: &Value, u: &str| {
        let all = v["projects"][0]["chunks"].as_array().unwrap();
        all.iter().find(|c| c["uid"] == u).unwrap().clone()
    };
    let v = board();
    assert_eq!(
        chunk(&v, &landed)["state"],
        "review",
        "a read changes nothing"
    );
    assert_eq!(chunk(&v, &landed)["merged"], true);
    assert!(chunk(&v, &waiting).get("merged").is_none());
    assert!(
        chunk(&v, &closed).get("merged").is_none(),
        "a gone upstream alone is not merged"
    );
    let branch = |n: &str| {
        let all = v["projects"][0]["branches"].as_array().unwrap();
        all.iter().find(|b| b["name"] == n).unwrap().clone()
    };
    assert_eq!(
        branch("feat/closed")["state"],
        "merged",
        "display unchanged"
    );

    assert_eq!(
        f(&["chunk", "accept", "--merged"]),
        format!("chunk {landed} → done")
    );
    let v = board();
    assert_eq!(chunk(&v, &landed)["state"], "done");
    assert_eq!(chunk(&v, &waiting)["state"], "review");
    assert_eq!(chunk(&v, &closed)["state"], "review");
    assert_eq!(
        f(&["chunk", "accept", "--merged"]),
        "no review chunk on a merged branch"
    );
}
