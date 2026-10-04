//! PLAN-fael-say-gate chunk 1: the golden replay. A fixture log with fixed ids
//! and a fixed run of events (session start → prompt → read → search → edit →
//! stop → compact → sub-agent → no session), every reply's `context` and
//! `notice` pinned byte for byte, plus the usage lines' event list and token
//! sum. A change here is a change to what the agent sees: on purpose it is
//! re-blessed with `FAEL_BLESS=1` and the diff reviewed in the PR.

use super::{fael_at, json};
use std::path::{Path, PathBuf};
use std::process::Command;

const SNAPSHOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/hook/golden/replay.txt");
/// The main session: an RFC 3339 time, so stop reads it as the session start.
const S: &str = "2025-12-01T00:00:00Z";
const BLOB_A: &str = "// a v1\n";

/// One fixture row, written straight into the log so ids and times are fixed.
fn row(id: &str, kind: &str, text: &str, files: &[&str], extra: &str) -> String {
    format!(
        r#"{{"v":1,"id":"{id}","ts":"2025-06-01T00:00:00.000Z","by":"fixture","kind":"{kind}","text":"{text}","files":{},"session":"old-session","branch":"old"{extra}}}"#,
        serde_json::to_string(files).unwrap()
    )
}

/// git's blob id of `bytes`, cut to the 12 hex digits `fh` stores.
fn blob(dir: &Path, file: &str) -> String {
    let o = Command::new("git")
        .args(["hash-object", file])
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim()[..12].to_string()
}

fn git(d: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(d)
            .status()
            .unwrap()
            .success()
    );
}

fn fixture() -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("replay-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["config", "user.name", "Replay"]);
    git(&d, &["config", "user.email", "replay@example.com"]);
    for (f, body) in [
        ("src/hub.rs", "// hub\n"),
        ("src/a.rs", BLOB_A),
        ("src/b.rs", "// b\n"),
        ("src/c.rs", "// c\n"),
        ("src/auth.rs", "// auth\n"),
    ] {
        std::fs::write(d.join(f), body).unwrap();
    }
    git(&d, &["add", "src"]);
    git(&d, &["commit", "-q", "-m", "init"]);
    std::fs::create_dir_all(d.join(".fael/log/fixture")).unwrap();
    std::fs::write(
        d.join(".fael/config.toml"),
        "store = \"tracked\"\n[sync]\nauto = false\n",
    )
    .unwrap();
    let same = format!(r#","fh":{{"src/a.rs":"{}"}}"#, blob(&d, "src/a.rs"));
    let changed = r#","fh":{"src/a.rs":"000000000000"}"#;
    let mut rows = vec![];
    // a hub file: more rows than the push cap, three kinds, two keys
    for i in 0..11 {
        let (kind, key) = match i % 3 {
            0 => ("decision", r#","key":"hub:shape""#),
            1 => ("issue", ""),
            _ => ("note", r#","key":"hub:wip""#),
        };
        rows.push(row(
            &format!("01K00000000000000000HUB{i:03}"),
            kind,
            &format!("hub row {i} says something about the hub"),
            &["src/hub.rs"],
            key,
        ));
    }
    rows.push(row(
        "01K00000000000000000AAA001",
        "decision",
        "a uses backoff, file changed since",
        &["src/a.rs"],
        changed,
    ));
    rows.push(row(
        "01K00000000000000000AAA002",
        "decision",
        "a keeps its cache, file unchanged",
        &["src/a.rs"],
        &same,
    ));
    rows.push(row(
        "01K00000000000000000BBB001",
        "issue",
        "b leaks a handle",
        &["src/b.rs"],
        "",
    ));
    rows.push(row(
        "01K00000000000000000CCC001",
        "note",
        "c is half migrated: the old reader still parses v0 rows and the new one skips them",
        &["src/c.rs"],
        r#","title":"c is half migrated""#,
    ));
    rows.push(row(
        "01K00000000000000000AUT001",
        "decision",
        "auth login retries twice",
        &["src/auth.rs"],
        r#","key":"auth:login""#,
    ));
    std::fs::write(
        d.join(".fael/log/fixture/2025-06.jsonl"),
        rows.join("\n") + "\n",
    )
    .unwrap();
    d
}

/// One scripted event: the hook name and its JSON body (`cwd` added here).
struct Step {
    hook: &'static str,
    body: String,
}

fn step(hook: &'static str, body: serde_json::Value) -> Step {
    Step {
        hook,
        body: body.to_string(),
    }
}

fn script() -> Vec<Step> {
    use serde_json::json;
    let read = |f: &str| json!({"session": S, "files": [f]});
    let edit = read;
    vec![
        step("session-start", json!({"session": S})),
        step(
            "prompt",
            json!({"session": S, "text": "fix the auth login flow"}),
        ),
        step(
            "prompt",
            json!({"session": S, "text": "fix the auth login flow"}),
        ),
        step("read", read("src/hub.rs")),
        step("read", read("src/hub.rs")),
        step("read", read("src/hub.rs")),
        step("read", read("src/hub.rs")),
        step(
            "search",
            json!({"session": S, "files": ["src/a.rs", "src/b.rs"]}),
        ),
        step("read", read("src/auth.rs")),
        step(
            "prompt",
            json!({"session": S, "text": "and the login again"}),
        ),
        step("edit", edit("src/a.rs")),
        step("edit", edit("src/a.rs")),
        step("edit", edit("src/b.rs")),
        step("edit", edit("src/b.rs")),
        step("edit", edit("src/hub.rs")),
        step("edit", edit("src/hub.rs")),
        step(
            "stop",
            json!({"session": S, "text": "the schema and the docs are out of sync",
                   "reply": "fael issue: broken thing with no files"}),
        ),
        step("read", read("src/nothing.rs")),
        step("read", read("src/nothing.rs")),
        step("session-start", json!({"session": S, "source": "compact"})),
        step("read", read("src/hub.rs")),
        step(
            "read",
            json!({"session": S, "agent": "sub1", "files": ["src/hub.rs"]}),
        ),
        step(
            "read",
            json!({"session": S, "agent": "sub1", "files": ["src/hub.rs"]}),
        ),
        step("read", json!({"files": ["src/hub.rs"]})),
        step("read", json!({"files": ["src/hub.rs"]})),
        step(
            "search",
            json!({"session": S, "tool": "Grep", "tool_input": {"path": "src/c.rs"}}),
        ),
        step(
            "search",
            json!({"session": S, "tool": "Bash",
                   "tool_input": {"command": "printf x >> src/b.rs"}}),
        ),
        step("session-start", json!({"session": "s2"})),
        step("read", json!({"session": "s2", "files": ["src/c.rs"]})),
        step("prompt", json!({"text": "fix the auth login flow"})),
    ]
}

/// Run the script; one block per step, then the usage lines' events and token
/// sum. Every path in the script is repo-relative: the snapshot never holds a
/// path whose spelling differs by OS.
fn replay() -> String {
    let d = fixture();
    let state = d.join(".state");
    let mut out = String::new();
    for (i, s) in script().into_iter().enumerate() {
        let mut body: serde_json::Value = serde_json::from_str(&s.body).unwrap();
        body["cwd"] = serde_json::from_str(&json(&d)).unwrap();
        let (ok, stdout, err) = fael_at(&state, &d, &["hook", s.hook], &body.to_string());
        assert!(ok, "step {i}: {err}");
        let r: serde_json::Value = serde_json::from_str(&stdout).expect(&stdout);
        out.push_str(&format!("## {i} {} {}\n", s.hook, s.body));
        for field in ["context", "notice"] {
            if let Some(t) = r[field].as_str() {
                out.push_str(&format!("{field}:\n{t}\n"));
            }
        }
    }
    let usage = std::fs::read_to_string(state.join("usage.jsonl")).unwrap_or_default();
    let (mut events, mut tokens) = (vec![], 0);
    for l in usage.lines() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        events.push(v["event"].as_str().unwrap_or("?").to_string());
        tokens += v["est_tokens"].as_u64().unwrap_or(0);
    }
    out.push_str(&format!(
        "## usage\n{} lines · ~{tokens} tokens\n{}\n",
        events.len(),
        events.join(" ")
    ));
    out.replace(&d.to_string_lossy().into_owned(), "<REPO>")
}

#[test]
fn replay_matches_golden() {
    let got = replay();
    if std::env::var_os("FAEL_BLESS").is_some() {
        std::fs::create_dir_all(Path::new(SNAPSHOT).parent().unwrap()).unwrap();
        std::fs::write(SNAPSHOT, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(SNAPSHOT).unwrap_or_default();
    assert!(
        got == want,
        "golden replay drifted — FAEL_BLESS=1 to re-bless on purpose\n--- got\n{got}"
    );
}
