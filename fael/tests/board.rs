//! `fael board --json` through the real binary against `tests/golden/board-v1.json` (SPEC
//! §10): every object carries the fixture's keys in its order, the four lists follow §5 and
//! §3, and the board runs from outside any repo off the registry, as the app runs it.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(dir: &Path, state: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .output()
        .unwrap()
}

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = run(dir, state, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn keys(v: &Value) -> Vec<&str> {
    v.as_object().unwrap().keys().map(String::as_str).collect()
}

fn fixture() -> Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/board-v1.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn repo(tmp: &Path) -> PathBuf {
    let d = tmp.join("repo");
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    d.canonicalize().unwrap()
}

fn write(p: PathBuf, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn the_binary_speaks_board_v1() {
    let tmp = std::env::temp_dir().join(format!("fael-board-{}", fael_core::ulid()));
    let (d, state) = (repo(&tmp), tmp.join("state"));
    let f = |args: &[&str]| fael(&d, &state, args);
    let a = f(&["chunk", "add", "a", "--brief", "b", "--scope", "src/"]);
    let b = f(&["chunk", "add", "b", "--brief", "b", "--after", &a]);
    let ask = f(&["chunk", "add", "ask", "--brief", "b"]);
    let free = f(&[
        "chunk", "add", "free", "--brief", "b", "--scope", "src/x.rs",
    ]);
    let idea = f(&["chunk", "add", "idea"]);
    let gone = f(&["chunk", "add", "gone", "--brief", "b"]);
    f(&["chunk", "start", &ask, "--run", "R0"]);
    f(&["chunk", "wait", &ask, "Which tone?", "--on", "owner"]);
    f(&["chunk", "start", &gone, "--run", "R1"]);
    f(&["run", "end", "R1"]);
    f(&["chunk", "start", &a, "--run", "R2", "--client", "claude"]);

    // the app runs it from home: the registry, not the cwd, names the repo
    let out = fael(&tmp, &state, &["board", "--json"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let g = fixture();
    assert_eq!(keys(&v), keys(&g));
    let (p, gp) = (&v["projects"][0], &g["projects"][0]);
    assert_eq!(keys(p), keys(gp));
    assert_eq!(p["root"], d.to_string_lossy().as_ref());
    assert_eq!(keys(&p["plans"][0]), keys(&gp["plans"][0]));
    assert_eq!(p["plans"][0]["counts"]["open"], 2);
    let chunk = |u: &str| {
        p["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["uid"] == u)
            .unwrap()
    };
    for c in p["chunks"].as_array().unwrap() {
        assert_eq!(keys(c), keys(&gp["chunks"][0]), "{c}");
    }
    assert_eq!(keys(&chunk(&a)["run"]), keys(&gp["chunks"][0]["run"]));
    assert_eq!(chunk(&a)["run"]["id"], "R2");
    assert_eq!(chunk(&a)["unblocks"], 1);
    assert_eq!(chunk(&a)["overlaps"], serde_json::json!([free]));
    assert_eq!(chunk(&ask)["handoff"]["text"], "Which tone?");
    let need: Vec<(&str, &str)> = v["needs_you"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| (n["kind"].as_str().unwrap(), n["uid"].as_str().unwrap()))
        .collect();
    let want = [
        ("waiting", ask.as_str()),
        ("ended", gone.as_str()),
        ("inbox", idea.as_str()),
    ];
    assert_eq!(need, want);
    assert_eq!(v["queue"], serde_json::json!([free]));
    assert_eq!(v["running"], serde_json::json!([a]));
    assert_eq!(v["blocked"], serde_json::json!([b]));
    // no linked worktree: the main checkout is the one slot, held by R2
    let wt = &p["worktrees"];
    assert_eq!(wt.as_array().unwrap().len(), 1);
    assert_eq!(keys(&wt[0]), keys(&gp["worktrees"][0]));
    assert_eq!(wt[0]["run"], "R2");

    let open: Value = serde_json::from_str(&f(&["board", "--json", "--open"])).unwrap();
    let states: Vec<&str> = open["projects"][0]["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["open", "open"], "b and free");
    let text = f(&["board"]);
    assert!(text.starts_with("needs you\n  waiting "), "{text}");
    assert!(text.contains("— Which tone?"), "{text}");
}

#[test]
fn a_linked_worktree_pool_and_the_registry() {
    let tmp = std::env::temp_dir().join(format!("fael-board-{}", fael_core::ulid()));
    let (d, state) = (repo(&tmp), tmp.join("state"));
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    git(
        &d,
        &[&id[..], &["commit", "-q", "--allow-empty", "-m", "init"]].concat(),
    );
    git(&d, &["worktree", "add", "-q", "../wt1", "-b", "w1"]);
    git(&d, &["worktree", "add", "-q", "../wt2", "-b", "w2"]);
    let wt1 = tmp.join("wt1").canonicalize().unwrap();
    let x = fael(&wt1, &state, &["chunk", "add", "x", "--brief", "b"]);
    fael(&wt1, &state, &["chunk", "start", &x, "--run", "R1"]);
    // written from a worktree, the registry names the main checkout
    let reg = std::fs::read_to_string(state.join("projects")).unwrap();
    assert_eq!(reg, format!("{}\n", d.display()));

    // a root gone from disk and a repo whose plans.db is not a database: both skipped
    let bad = tmp.join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    git(&bad, &["init", "-q", "-b", "main"]);
    let bad = bad.canonicalize().unwrap();
    write(bad.join(".git/fael/plans.db"), "not sqlite");
    let gone = tmp.join("gone");
    write(
        state.join("projects"),
        &format!("{}\n{}\n{reg}", gone.display(), bad.display()),
    );
    let o = run(&tmp, &state, &["board", "--json"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        o.status.success() && err.contains(&*bad.to_string_lossy()),
        "{err}"
    );
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["projects"].as_array().unwrap().len(), 1, "{v}");
    let p = &v["projects"][0];
    assert_eq!(p["root"], d.to_string_lossy().as_ref());
    // linked worktrees only: the main checkout is not a slot
    let wt = &p["worktrees"];
    let slots: Vec<(&str, &str, &Value)> = wt
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                w["path"].as_str().unwrap(),
                w["branch"].as_str().unwrap(),
                &w["run"],
            )
        })
        .collect();
    let wt2 = tmp.join("wt2").canonicalize().unwrap();
    assert_eq!(
        slots,
        [
            (&*wt1.to_string_lossy(), "w1", &Value::from("R1")),
            (&*wt2.to_string_lossy(), "w2", &Value::Null),
        ]
    );
}

#[test]
fn an_md_plan_is_never_ready_and_a_done_plan_shows_counts() {
    let tmp = std::env::temp_dir().join(format!("fael-board-{}", fael_core::ulid()));
    let (d, state) = (repo(&tmp), tmp.join("state"));
    write(
        d.join(".fapony/plan/PLAN-m.md"),
        "# PLAN-m — live\n\n## TL;DR\n- [ ] m1 — open one\n",
    );
    write(
        d.join(".fapony/done/PLAN-old.md"),
        "# PLAN-old\n\n## TL;DR\n- [x] o1 — one\n- [x] o2 — two\n",
    );
    fael(&d, &state, &["plan", "import"]);
    let v: Value = serde_json::from_str(&fael(&tmp, &state, &["board", "--json"])).unwrap();
    let p = &v["projects"][0];
    let plans: Vec<(&str, &str, &str)> = p["plans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let s = |k: &str| p[k].as_str().unwrap();
            (s("name"), s("state"), s("truth"))
        })
        .collect();
    assert_eq!(plans, [("m", "active", "md"), ("old", "done", "md")]);
    assert_eq!(p["plans"][0]["title"], "live");
    assert_eq!(p["plans"][1]["counts"], serde_json::json!({"done": 2}));
    let chunks = p["chunks"].as_array().unwrap();
    assert_eq!(chunks.len(), 1, "the done plan lists no chunk");
    assert_eq!(chunks[0]["title"], "open one");
    assert_eq!(chunks[0]["ready"], false, "only a db plan's chunk starts");
    assert_eq!(v["queue"], serde_json::json!([]));
}
