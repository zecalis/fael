//! A plan whose code is in another checkout (issue 01M4N4CVH): `fael plan repo` names it,
//! the brief says so, the board lists the checkout as a project and gives the plan its
//! remote, `start --worktree` runs in a worktree of it (issue 01M4NC9GV), and `done --pr`
//! takes the PR's URL and refuses another repo's (issue 01M4QFJ6V).

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(dir: &Path, state: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap()
}

fn ok(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = run(dir, state, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(d: &Path, args: &[&str]) {
    let s = Command::new("git").args(args).current_dir(d).status();
    assert!(s.unwrap().success(), "git {args:?}");
}

fn repo(tmp: &Path, name: &str) -> PathBuf {
    let d = tmp.join(name);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    d.canonicalize().unwrap()
}

#[test]
fn a_plan_in_another_checkout() {
    let t = std::env::temp_dir().join(format!("fael-cross-{}", fael_core::ulid()));
    let (d, code, state) = (repo(&t, "fael"), repo(&t, "board"), t.join("state"));
    git(
        &code,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:zecalis/fael-board.git",
        ],
    );
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    git(
        &code,
        &[&id[..], &["commit", "-q", "--allow-empty", "-m", "i"]].concat(),
    );
    git(&code, &["worktree", "add", "-q", "../board-wt", "-b", "w"]);
    let f = |args: &[&str]| ok(&d, &state, args);
    let a = f(&["chunk", "add", "a", "--brief", "b"]);
    let bad = run(&d, &state, &["plan", "repo", "inbox", "../nope"]);
    assert!(!bad.status.success(), "not a checkout");
    let to = f(&["plan", "repo", "inbox", "../board"]);
    assert!(to.ends_with(&format!("code in {}", code.display())), "{to}");

    let v: Value = serde_json::from_str(&ok(&t, &state, &["board", "--json"])).unwrap();
    let ps = v["projects"].as_array().unwrap();
    let plan = &ps[0]["plans"][0];
    assert_eq!(plan["repo"], code.to_string_lossy().as_ref());
    assert_eq!(plan["remote"], "zecalis/fael-board");
    let p = &ps[1];
    assert_eq!(p["root"], code.to_string_lossy().as_ref());
    assert_eq!(p["plans"], serde_json::json!([]));
    assert_eq!(p["worktrees"].as_array().unwrap().len(), 1, "{p}");

    let wt = t.join("board-wt").canonicalize().unwrap();
    let w = wt.to_string_lossy();
    let both = run(
        &d,
        &state,
        &["chunk", "start", &a, "--worktree", &w, "--no-worktree"],
    );
    assert!(!both.status.success(), "--worktree or --no-worktree");
    let brief = f(&["chunk", "start", &a, "--run", "R1", "--worktree", &w]);
    let at = format!(
        "run `fael chunk` from {}, wait/after/done with --run R1",
        d.display()
    );
    assert!(
        brief.contains(&format!("code in {}", code.display())),
        "{brief}"
    );
    assert!(brief.contains(&at), "{brief}");
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &a, "--json"])).unwrap();
    assert_eq!(
        (&c["run"]["worktree"], &c["run"]["branch"]),
        (&w.as_ref().into(), &"w".into())
    );
    let other = run(
        &d,
        &state,
        &[
            "chunk",
            "done",
            &a,
            "h",
            "--pr",
            "zecalis/fael#7",
            "--run",
            "R1",
        ],
    );
    let err = String::from_utf8_lossy(&other.stderr);
    assert!(
        err.contains("is not this plan's repo (zecalis/fael-board)"),
        "{err}"
    );
    let url = "https://github.com/Zecalis/fael-board/pull/7";
    f(&["chunk", "done", &a, "h", "--pr", url, "--run", "R1"]);
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &a, "--json"])).unwrap();
    assert_eq!(c["run"]["pr"], 7);

    f(&["plan", "repo", "inbox", "-"]);
    let v: Value = serde_json::from_str(&ok(&t, &state, &["board", "--json"])).unwrap();
    assert_eq!(v["projects"].as_array().unwrap().len(), 1);
}
