//! The plan-board reads an agent needs (PLAN-fael-board, issues of 2026-10-11): an
//! unreadable plans.db is never an empty board, `--plan` cuts it, `chunk show` reads one
//! chunk by uid or label, a find matches a key, and a chunk uid is no phantom citation.

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

fn ok(o: Output) -> String {
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    String::from_utf8_lossy(&o.stdout).trim().to_string() + &err
}

fn repo(tmp: &Path, name: &str) -> PathBuf {
    let d = tmp.join(name);
    std::fs::create_dir_all(&d).unwrap();
    let git = |args: &[&str]| {
        let s = Command::new("git").args(args).current_dir(&d).status();
        assert!(s.unwrap().success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "T"]);
    git(&["config", "user.email", "t@example.com"]);
    d.canonicalize().unwrap()
}

fn tmp() -> PathBuf {
    std::env::temp_dir().join(format!("fael-board-read-{}", fael_core::ulid()))
}

#[test]
fn an_unreadable_plans_db_stays_on_the_board_and_fails_it() {
    let t = tmp();
    let (good, bad, state) = (repo(&t, "good"), repo(&t, "bad"), t.join("state"));
    ok(run(&good, &state, &["chunk", "add", "a", "--brief", "b"]));
    ok(run(&bad, &state, &["chunk", "add", "b", "--brief", "b"]));
    let db = bad.join(".git/fael/plans.db");
    // user_version = 99: the 4 big-endian bytes at offset 60 of the header (fael's last
    // close checkpointed the WAL into the file)
    let mut bytes = std::fs::read(&db).unwrap();
    bytes[60..64].copy_from_slice(&99u32.to_be_bytes());
    std::fs::write(&db, bytes).unwrap();
    let o = run(&t, &state, &["board", "--json"]);
    assert!(
        !o.status.success(),
        "a project fael cannot read fails the board"
    );
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    let ps = v["projects"].as_array().unwrap();
    let by = |root: &Path| {
        ps.iter()
            .find(|p| p["root"] == root.to_string_lossy().as_ref())
    };
    let b = by(&bad).unwrap();
    assert!(b["error"].as_str().unwrap().contains("schema v99"), "{b}");
    assert_eq!(b["chunks"], serde_json::json!([]));
    let g = by(&good).unwrap();
    assert!(
        g.get("error").is_none(),
        "a readable project has no error key"
    );
    assert_eq!(g["chunks"].as_array().unwrap().len(), 1);
    let text = run(&t, &state, &["board"]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("unreadable "));
}

#[test]
fn labels_show_and_the_plan_cut() {
    let t = tmp();
    let (d, state) = (repo(&t, "r"), t.join("state"));
    let f = |args: &[&str]| ok(run(&d, &state, args));
    let a = f(&[
        "chunk",
        "add",
        "board app",
        "--brief",
        "the brief",
        "--label",
        "b10",
    ]);
    let a = a.lines().next().unwrap().to_string();
    let dup = run(&d, &state, &["chunk", "add", "x", "--label", "b10"]);
    assert!(!dup.status.success(), "a taken label is rejected");
    f(&["chunk", "note", "b10", "use the fixture"]);
    let v: Value = serde_json::from_str(&f(&["chunk", "show", "b10", "--json"])).unwrap();
    assert_eq!(v["uid"], a.as_str());
    assert_eq!(v["label"], "b10");
    assert_eq!(v["brief"], "the brief");
    assert_eq!(v["notes"], serde_json::json!(["use the fixture"]));
    let text = f(&["chunk", "show", &a]);
    assert!(
        text.starts_with(&format!("{a}  inbox b10  open  board app")),
        "{text}"
    );
    assert!(text.contains("> the brief") && text.contains("note: use the fixture"));
    let cut = |p: &str| -> Value {
        serde_json::from_str(&ok(run(&t, &state, &["board", "--json", "--plan", p]))).unwrap()
    };
    assert_eq!(cut("inbox")["projects"][0]["chunks"][0]["uid"], a.as_str());
    assert_eq!(cut("nope")["projects"], serde_json::json!([]));
}

#[test]
fn find_matches_a_key_and_a_chunk_uid_is_no_phantom() {
    let t = tmp();
    let (d, state) = (repo(&t, "r"), t.join("state"));
    let f = |args: &[&str]| ok(run(&d, &state, args));
    std::fs::write(d.join("a.rs"), "").unwrap();
    let uid = f(&["chunk", "add", "c", "--brief", "b"]);
    let uid = uid.lines().next().unwrap();
    let out = f(&[
        "add",
        "issue",
        &format!("Board goes blank, seen on chunk {uid}"),
        "--files",
        "a.rs",
        "--key",
        "board:schema-silent",
    ]);
    assert!(!out.contains("no row with id"), "{out}");
    let hit = f(&["find", "schema-silent"]);
    assert!(hit.contains("Board goes blank"), "{hit}");
}

#[test]
fn a_bound_session_is_seen_by_its_stop_hook_in_another_repo() {
    let t = tmp();
    let (d, other, state) = (repo(&t, "r"), repo(&t, "code"), t.join("state"));
    let uid = ok(run(&d, &state, &["chunk", "add", "c", "--brief", "b"]));
    let uid = uid.lines().next().unwrap();
    let start = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["chunk", "start", uid, "--no-worktree"])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", &state)
        .env("CLAUDE_CODE_SESSION_ID", "sess-1")
        .output()
        .unwrap();
    ok(start);
    let show = |k: &str| -> Value {
        serde_json::from_str(&ok(run(&d, &state, &["chunk", "show", uid, "--json"])))
            .map(|v: Value| v["run"][k].clone())
            .unwrap()
    };
    assert_eq!(show("session"), "sess-1");
    assert_eq!(show("last_seen"), Value::Null);
    // Claude keys the hook by the transcript path; its stem is the session id
    let ev = serde_json::json!({
        "cwd": other, "session": "/x/.claude/projects/p/sess-1.jsonl", "text": "ok"
    });
    let mut stop = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["hook", "stop"])
        .current_dir(&other)
        .env("FAEL_STATE_DIR", &state)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::io::Write::write_all(stop.stdin.as_mut().unwrap(), ev.to_string().as_bytes()).unwrap();
    assert!(stop.wait().unwrap().success());
    assert!(show("last_seen").is_string(), "stamped from another cwd");
}
