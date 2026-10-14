//! `fael chunk …` / `fael run end` through the real binary: the launcher's shell line
//! (`b=$(fael chunk start <uid> --run R) && { claude "$b"; fael run end R; }`) and `--out`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-chunk-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    d
}

fn add(d: &Path) -> String {
    let (ok, out, err) = fael(d, &["chunk", "add", "Post", "--brief", "Write it"]);
    assert!(ok, "{err}");
    out.trim().to_string()
}

#[test]
fn a_lost_start_prints_no_brief_and_its_run_end_ends_nothing() {
    let d = repo();
    let u = add(&d);
    let (ok, brief, _) = fael(
        &d,
        &["chunk", "start", &u, "--run", "R1", "--client", "claude"],
    );
    assert!(
        ok && brief.starts_with("fael run R1 · 1 chunk\n"),
        "{brief}"
    );
    let (ok, out, err) = fael(&d, &["chunk", "start", &u, "--run", "R2"]);
    assert!(!ok && out.is_empty(), "the shell opens no agent: {out}");
    assert!(err.contains("held by run R1 (claude)"), "{err}");
    // the loser's cleanup: exit 0, R1 still holds
    assert!(fael(&d, &["run", "end", "R2"]).0);
    let (_, _, err) = fael(&d, &["chunk", "start", &u, "--run", "R3"]);
    assert!(err.contains("held by run R1"), "{err}");
    assert!(fael(&d, &["run", "end", "R1"]).0);
    assert!(fael(&d, &["run", "end", "R1"]).0, "none left is fine");
    let (ok, _, err) = fael(&d, &["chunk", "wait", &u, "Which tone?"]);
    assert!(!ok && err.contains("--on owner|data"), "{err}");
    let (ok, out, err) = fael(&d, &["chunk", "wait", &u, "Which tone?", "--on", "owner"]);
    assert!(ok, "{err}");
    assert_eq!(out, format!("chunk {u} → waiting on owner\n"));
    let (_, out, _) = fael(&d, &["chunk", "answer", &u, "Warm"]);
    assert_eq!(out, format!("chunk {u} → open\n"));
    let (_, brief, _) = fael(&d, &["chunk", "start", &u]);
    assert!(brief.contains("owner said:\n> Warm\n"), "{brief}");
    assert!(!fael(&d, &["chunk", "bogus", &u]).0);
}

#[test]
fn done_out_copies_the_output_per_run() {
    let d = repo();
    let u = add(&d);
    assert!(fael(&d, &["chunk", "start", &u, "--run", "R1"]).0);
    let (ok, _, err) = fael(&d, &["chunk", "done", &u, "h", "--out", "nope.png"]);
    assert!(!ok && err.contains("does not exist"), "{err}");
    std::fs::create_dir_all(d.join("out/img")).unwrap();
    std::fs::write(d.join("out/img/a.png"), "png").unwrap();
    let (ok, _, err) = fael(&d, &["chunk", "done", &u, "two images", "--out", "out/img"]);
    assert!(ok, "{err}");
    let kept = d.join(".git/fael/out").join(&u).join("R1/img/a.png");
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "png");
    let (_, out, _) = fael(&d, &["plan", "export", "inbox"]);
    assert!(out.contains(" · review"), "{out}");
    let (_, out, _) = fael(&d, &["chunk", "accept", &u]);
    assert_eq!(out, format!("chunk {u} → done\n"));
}

/// `fael hook <event>` with this worktree as cwd: the event reaches fael.db.
fn hook(d: &Path, event: &str, session: &str) {
    use std::io::Write as _;
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["hook", event])
        .current_dir(d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("FAEL_DIR")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let e = serde_json::json!({"cwd": d, "session": session, "text": ""});
    c.stdin
        .take()
        .unwrap()
        .write_all(e.to_string().as_bytes())
        .unwrap();
    assert!(c.wait().unwrap().success());
}

#[test]
fn the_stop_hook_stamps_its_worktree_run() {
    let d = repo();
    let u = add(&d);
    assert!(fael(&d, &["chunk", "start", &u, "--run", "R1"]).0);
    hook(&d, "stop", "2026-10-11T09:00:00Z");
    // the hook filled `session`: another session's stamp now finds no run
    let wt = d.canonicalize().unwrap().to_string_lossy().into_owned();
    let mut s = fael_core::plan::Store::open(&d.join(".git/fael/fael.db")).unwrap();
    assert_eq!(s.seen(&wt, "2026-10-11T10:00:00Z", "t").unwrap(), 0);
    assert_eq!(s.seen(&wt, "2026-10-11T09:00:00Z", "t").unwrap(), 1);
}

#[test]
fn a_run_killed_mid_turn_resumes_from_its_session_notes_and_branch() {
    let d = repo();
    let u = add(&d);
    assert!(
        fael(
            &d,
            &["chunk", "start", &u, "--run", "R1", "--client", "claude"]
        )
        .0
    );
    // session start stamps the run before any Stop: a kill mid-turn keeps the session
    hook(&d, "session-start", "S1");
    assert!(fael(&d, &["chunk", "note", &u, "parser done; next the CLI"]).0);
    let (ok, brief, err) = fael(&d, &["chunk", "start", &u, "--run", "R2", "--force"]);
    assert!(ok, "{err}");
    assert!(
        brief.contains("\nresume — run R1 (claude) started "),
        "{brief}"
    );
    assert!(brief.contains("\nsession S1\n"), "{brief}");
    assert!(
        brief.contains("`git log origin/main..main` · `git status`"),
        "{brief}"
    );
    assert!(brief.contains("> parser done; next the CLI\n"), "{brief}");
    // a plain start of a fresh chunk has nothing to resume
    let (_, fresh, _) = fael(&d, &["chunk", "start", &add(&d), "--no-worktree"]);
    assert!(!fresh.contains("resume —"), "{fresh}");
}

/// An agent's own start binds its session: `done --pr` in the same turn ends the run
/// before any Stop could stamp it, and the board still has the session to resume (01M4NXDQ).
#[test]
fn an_agent_start_keeps_its_session_through_done() {
    let d = repo();
    let u = add(&d);
    let with = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fael"))
            .args(args)
            .current_dir(&d)
            .env("FAEL_STATE_DIR", d.join("state"))
            .env_remove("FAEL_DIR")
            .env("CLAUDE_CODE_SESSION_ID", "S9")
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(with(&["chunk", "start", &u]));
    assert!(with(&["chunk", "done", &u, "shipped", "--pr", "7"]));
    let (_, out, _) = fael(&d, &["board", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["projects"][0]["chunks"][0]["run"]["session"], "S9", "{v}");
}

#[test]
fn apply_reads_stdin_and_the_board_sees_the_approval() {
    let d = repo();
    let (a, b) = (add(&d), add(&d));
    let apply = |json: String| {
        use std::io::Write;
        let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
            .args(["chunk", "apply", "--json", "-"])
            .current_dir(&d)
            .env("FAEL_STATE_DIR", d.join("state"))
            .env_remove("FAEL_DIR")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(json.as_bytes()).unwrap();
        let o = c.wait_with_output().unwrap();
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    let (ok, out) = apply(format!(r#"[{{"op":"pair","chunks":["{a}","{b}"]}}]"#));
    assert!(ok && out == format!("paired {a}, {b}\n"), "{out}");
    assert!(!apply("{}".into()).0, "an object, not an array");
    let (ok, _, err) = fael(&d, &["chunk", "approve", &a]);
    assert!(!ok && err.contains("pass --model M"), "{err}");
    // the client defaults to claude
    let (ok, out, _) = fael(&d, &["chunk", "approve", &a, "--model", "opus"]);
    assert!(
        ok && out.contains("approved · model opus · client claude"),
        "{out}"
    );
    // pin is gone with epoch v8: the queue is plan order and unblocks
    assert!(!fael(&d, &["chunk", "pin", &b, "1"]).0);
    let (_, json, _) = fael(&d, &["board", "--json"]);
    assert!(
        json.contains(r#""model":"opus""#) || json.contains(r#""model": "opus""#),
        "{json}"
    );
}

/// Issue 01M4XM98: the planner's client rides on the chunk, and approve falls back to it.
#[test]
fn a_client_hint_is_what_approve_takes_without_client() {
    let d = repo();
    let add = ["chunk", "add", "Ui", "--brief", "b", "--model", "haiku"];
    let (ok, out, err) = fael(&d, &[&add[..], &["--client", "opencode"]].concat());
    assert!(ok, "{err}");
    let u = out.trim().to_string();
    let (_, json, _) = fael(&d, &["board", "--json"]);
    assert!(json.contains(r#""client_hint":"opencode""#), "{json}");
    let (ok, out, _) = fael(&d, &["chunk", "approve", &u]);
    assert!(ok && out.contains("model haiku · client opencode"), "{out}");
    // an empty --client clears the hint
    assert!(fael(&d, &["chunk", "edit", &u, "--client", ""]).0);
    let (ok, out, _) = fael(&d, &["chunk", "approve", &u]);
    assert!(ok && out.contains("model haiku · client claude"), "{out}");
}

/// Issue 01M4XN3S: a start without flags runs with the approval's model and client, and
/// done keeps the review's outcome on that run.
#[test]
fn a_bare_start_takes_the_approval_and_done_keeps_the_review() {
    let d = repo();
    let (_, out, _) = fael(&d, &["chunk", "add", "Ui", "--brief", "b"]);
    let u = out.trim().to_string();
    let approve = [
        "chunk", "approve", &u, "--model", "oc/m", "--client", "opencode",
    ];
    assert!(fael(&d, &approve).0);
    assert!(fael(&d, &["chunk", "start", &u, "--run", "R1"]).0);
    let (ok, _, err) = fael(&d, &["chunk", "done", &u, "h", "--review", "meh"]);
    assert!(
        !ok && err.contains("--review is clean|fixed|failed"),
        "{err}"
    );
    assert!(fael(&d, &["chunk", "done", &u, "h", "--review", "fixed"]).0);
    let (_, json, _) = fael(&d, &["board", "--json"]);
    let b: serde_json::Value = serde_json::from_str(&json).unwrap();
    let run = &b["projects"][0]["chunks"][0]["run"];
    assert_eq!(
        (&run["client"], &run["model"], &run["review"]),
        (&"opencode".into(), &"oc/m".into(), &"fixed".into()),
        "{run}"
    );
}

#[test]
fn an_image_chunk_starts_without_a_worktree_and_its_shell_finishes_it() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &["chunk", "add", "Hero", "--brief", "Draw it", "--size", "S"],
    );
    assert!(ok, "{err}");
    let u = out.trim().to_string();
    let (ok, _, err) = fael(&d, &["chunk", "edit", &u, "--size", "XL"]);
    assert!(!ok && err.contains("S, M or L"), "{err}");
    let (ok, _, err) = fael(
        &d,
        &[
            "chunk",
            "start",
            &u,
            "--run",
            "R1",
            "--model",
            "flux",
            "--no-worktree",
        ],
    );
    assert!(ok, "{err}");
    // the worktree is still free: an agent starts here
    let other = add(&d);
    assert!(fael(&d, &["chunk", "start", &other, "--run", "R2"]).0);
    // the launching shell, from a subdir, names its run
    let sub = d.join("gen");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("a.png"), "png").unwrap();
    let (ok, _, err) = fael(&sub, &["chunk", "done", &u, "h", "--run", "R0"]);
    assert!(!ok && err.contains("not R0"), "{err}");
    let (ok, out, err) = fael(
        &sub,
        &[
            "chunk", "done", &u, "drawn", "--run", "R1", "--out", "a.png",
        ],
    );
    assert!(ok, "{err}");
    assert_eq!(out, format!("chunk {u} → review\n"));
    let (_, json, _) = fael(&d, &["board", "--json"]);
    let b: serde_json::Value = serde_json::from_str(&json).unwrap();
    let c = b["projects"][0]["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["uid"] == u.as_str())
        .unwrap();
    assert_eq!(
        (&c["run"]["model"], &c["run"]["worktree"]),
        (&"flux".into(), &serde_json::Value::Null),
        "{c}"
    );
}

#[test]
fn a_scope_naming_nothing_is_warned_and_a_new_file_in_a_real_dir_is_not() {
    let d = repo();
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/write.rs"), "").unwrap();
    let scope = "src/write.rs,src/new.rs,src/wirte.rs,nope/a.rs,docs/";
    let (ok, out, err) = fael(&d, &["chunk", "add", "T", "--brief", "b", "--scope", scope]);
    assert!(ok, "{err}");
    let uid = out.trim();
    assert!(
        err.contains(r#"scope "src/wirte.rs" is not on disk — "src/write.rs"?"#),
        "{err}"
    );
    assert!(err.contains(r#"scope "nope/a.rs" names nothing"#), "{err}");
    for quiet in ["\"src/write.rs\" ", "src/new.rs", "docs/"] {
        assert!(!err.contains(quiet), "{quiet}: {err}");
    }
    let (ok, _, err) = fael(&d, &["chunk", "edit", uid, "--title", "U"]);
    assert!(ok && !err.contains("warning"), "{err}");
    let (ok, _, err) = fael(&d, &["chunk", "edit", uid, "--scope", "nope/sub/"]);
    assert!(
        ok && err.contains(r#"scope "nope/sub/" names nothing"#),
        "{err}"
    );
}

#[test]
fn a_wait_on_the_owner_offers_its_options() {
    let d = repo();
    let u = add(&d);
    assert!(fael(&d, &["chunk", "start", &u, "--run", "R1"]).0);
    let data = ["chunk", "wait", &u, "q", "--on", "data", "--option", "A"];
    let (ok, _, err) = fael(&d, &data);
    assert!(!ok && err.contains("--option needs --on owner"), "{err}");
    // issue 01M4NWFS: in order, a comma kept inside one
    let (warm, dry) = ("--option=Warm, plain", "--option=Dry");
    let (ok, _, err) = fael(
        &d,
        &["chunk", "wait", &u, "Tone?", "--on", "owner", warm, dry],
    );
    assert!(ok, "{err}");
    let (_, json, _) = fael(&d, &["chunk", "show", &u, "--json"]);
    let c: serde_json::Value = serde_json::from_str(&json).unwrap();
    let keys: Vec<&String> = c["wait"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["on", "text", "until", "options"]);
    assert_eq!(
        c["wait"]["options"],
        serde_json::json!(["Warm, plain", "Dry"])
    );
}
