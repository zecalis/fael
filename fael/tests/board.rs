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
        // the owner's app reads with no agent session — inside one, a call
        // line (friction accounting) is the one write a board read makes
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
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

/// A path fael printed, as the test spells it: git prints `C:/…`, canonicalize `\\?\C:\…`.
fn canon(p: &str) -> PathBuf {
    Path::new(p.trim_end()).canonicalize().unwrap()
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
    let bad = run(&d, &state, &["run", "end", "R1", "--cost", "-1"]);
    assert!(!bad.status.success(), "a negative cost is rejected");
    f(&["run", "end", "R1", "--cost", "0.42"]);
    f(&["chunk", "start", &a, "--run", "R2", "--client", "claude"]);
    write(d.join(".fael/shared"), "# every lane\ndocs/\n");

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
    assert_eq!(chunk(&a)["run"]["cost"], Value::Null);
    assert_eq!(chunk(&gone)["run"]["cost"], 0.42);
    assert_eq!(chunk(&a)["unblocks"], 1);
    assert_eq!(chunk(&a)["overlaps"], serde_json::json!([free]));
    assert_eq!(p["shared"], serde_json::json!(["docs/"]));
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

    // a shared path is nobody's claim: src/x.rs no longer makes a and free overlap
    write(d.join(".fael/shared"), "src/x.rs\n");
    let v: Value = serde_json::from_str(&f(&["board", "--json"])).unwrap();
    assert!(
        v["projects"][0]["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["overlaps"] == serde_json::json!([]))
    );
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
    assert_eq!((canon(&reg), reg.lines().count()), (d.clone(), 1));

    // a root gone from disk is skipped; a fael.db that is not a database stays listed
    // with its error and fails the board (issue 01M4N4CV7)
    let bad = tmp.join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    git(&bad, &["init", "-q", "-b", "main"]);
    let bad = bad.canonicalize().unwrap();
    write(bad.join(".git/fael/fael.db"), "not sqlite");
    let gone = tmp.join("gone");
    write(
        state.join("projects"),
        &format!("{}\n{}\n{reg}", gone.display(), bad.display()),
    );
    let o = run(&tmp, &state, &["board", "--json"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        !o.status.success() && err.contains(&*bad.to_string_lossy()),
        "{err}"
    );
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let ps = v["projects"].as_array().unwrap();
    assert_eq!(ps.len(), 2, "{v}");
    assert_eq!(canon(ps[0]["root"].as_str().unwrap()), bad);
    assert!(ps[0]["error"].is_string(), "{v}");
    let p = &ps[1];
    assert_eq!(canon(p["root"].as_str().unwrap()), d);
    // linked worktrees only: the main checkout is not a slot
    let wt = &p["worktrees"];
    let slots: Vec<(PathBuf, &str, &Value)> = wt
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                canon(w["path"].as_str().unwrap()),
                w["branch"].as_str().unwrap(),
                &w["run"],
            )
        })
        .collect();
    let wt2 = tmp.join("wt2").canonicalize().unwrap();
    assert_eq!(
        slots,
        [
            (wt1, "w1", &Value::from("R1")),
            (wt2.clone(), "w2", &Value::Null),
        ]
    );
    // `[plan] worktrees` keeps the agent slots, leaves a topic worktree out
    write(state.join("projects"), &reg);
    write(d.join(".fael/config.toml"), "[plan]\nworktrees = \"wt2\"\n");
    let v: Value = serde_json::from_str(&fael(&tmp, &state, &["board", "--json"])).unwrap();
    let wt = v["projects"][0]["worktrees"].as_array().unwrap().clone();
    assert_eq!(wt.len(), 1, "{wt:?}");
    assert_eq!(canon(wt[0]["path"].as_str().unwrap()), wt2);

    // --drop takes a finished repo off the board; a root not on it is rejected
    let gone = gone.to_string_lossy();
    assert!(
        !run(&tmp, &state, &["board", "--drop", &gone])
            .status
            .success()
    );
    fael(&tmp, &state, &["board", "--drop", &d.to_string_lossy()]);
    assert_eq!(std::fs::read_to_string(state.join("projects")).unwrap(), "");
}

#[test]
fn a_temp_repo_stays_off_a_real_registry() {
    let tmp = std::env::temp_dir().join(format!("fael-board-{}", fael_core::ulid()));
    let d = repo(&tmp);
    // a state dir outside the temp dir stands in for the owner's
    let state = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fael-board-{}", fael_core::ulid()))
        .join("state");
    fael(&d, &state, &["chunk", "add", "x", "--brief", "b"]);
    let reg = state.join("projects");
    assert!(!reg.exists(), "a scratch repo is never registered");
    // one an older fael wrote is skipped and pruned
    write(reg.clone(), &format!("{}\n", d.display()));
    let v: Value = serde_json::from_str(&fael(&tmp, &state, &["board", "--json"])).unwrap();
    assert_eq!(v["projects"], serde_json::json!([]));
    assert_eq!(std::fs::read_to_string(&reg).unwrap(), "");
}

#[test]
fn a_done_plan_shows_counts_and_lists_no_chunk() {
    let tmp = std::env::temp_dir().join(format!("fael-board-{}", fael_core::ulid()));
    let (d, state) = (repo(&tmp), tmp.join("state"));
    let f = |args: &[&str]| fael(&d, &state, args);
    f(&["plan", "new", "old", "Old work"]);
    let uid = f(&["chunk", "add", "o1", "--plan", "old", "--brief", "b"]);
    let uid = uid.split_whitespace().next().unwrap();
    f(&["chunk", "start", uid, "--no-worktree"]);
    f(&["chunk", "done", uid, "shipped", "--pr", "1"]);
    f(&["plan", "done", "old"]);
    let v: Value = serde_json::from_str(&fael(&tmp, &state, &["board", "--json"])).unwrap();
    let p = &v["projects"][0];
    let old = p["plans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "old")
        .unwrap();
    // board JSON v1: truth stays, a constant the app reads
    assert_eq!(
        (&old["state"], &old["truth"]),
        (&"done".into(), &"db".into())
    );
    assert_eq!(old["counts"], serde_json::json!({"done": 1}));
    assert!(
        p["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["plan"] != "old"),
        "the done plan lists no chunk: {p}"
    );
}

/// Every file under `dir` with its bytes: what a run wrote to the state dir.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(snapshot(&p));
        } else {
            out.push((p.clone(), std::fs::read(&p).unwrap()));
        }
    }
    out.sort();
    out
}

/// PLAN-fael-board l1: the open ledger rides the board, issues grouped by shared files, and
/// the read leaves no trace — no find recorded, no row marked seen (issue 01M4MSMF).
#[test]
fn the_ledger_rides_the_board_and_reads_like_nothing() {
    let tmp = std::env::temp_dir().join(format!("fael-board-{}", fael_core::ulid()));
    let (d, state) = (repo(&tmp), tmp.join("state"));
    for f in ["a.rs", "b.rs", "c.rs"] {
        write(d.join("src").join(f), "");
    }
    let f = |args: &[&str]| fael(&d, &state, args);
    let id = |out: String| out.split_whitespace().next().unwrap().to_string();
    let a = id(f(&["add", "issue", "a breaks", "--files", "src/a.rs"]));
    let ab = id(f(&[
        "add",
        "issue",
        "a and b drift",
        "--files",
        "src/a.rs,src/b.rs",
    ]));
    let c = id(f(&["add", "issue", "c is slow", "--files", "src/c.rs"]));
    let old = id(f(&["add", "issue", "c was odd", "--files", "src/c.rs"]));
    f(&["close", &old, "fixed"]);
    let dec = id(f(&[
        "add",
        "decision",
        "keep c small",
        "--files",
        "src/c.rs",
        "--key",
        "c:size",
    ]));
    let note = id(f(&["add", "note", "next: b", "--files", "src/b.rs"]));
    f(&["chunk", "add", "x", "--brief", "b"]);

    let before = snapshot(&state);
    let v: Value = serde_json::from_str(&fael(&tmp, &state, &["board", "--json"])).unwrap();
    assert!(before == snapshot(&state), "board wrote to the state dir");
    let l = &v["projects"][0]["ledger"];
    let rows = |k: &str| -> Vec<(String, Option<u64>)> {
        l[k].as_array()
            .unwrap()
            .iter()
            .map(|r| (r["id"].as_str().unwrap().to_string(), r["group"].as_u64()))
            .collect()
    };
    let mut issues = rows("issues");
    issues[..2].sort();
    let mut want = vec![(a, Some(0)), (ab, Some(0))];
    want.sort();
    want.push((c, Some(1)));
    assert_eq!(
        issues, want,
        "a shared file is one group; a closed issue is gone"
    );
    assert_eq!(rows("decisions"), [(dec, None)]);
    assert_eq!(rows("notes"), [(note, None)]);
    assert_eq!(l["decisions"][0]["key"], "c:size");
    assert_eq!(l["notes"][0]["signals"], serde_json::json!([]));
}
