//! A hand-off between agents: `--to <client>` and the receipt line.

use super::clients::{claude_fixture, codex_fixture};
use super::{fael, fael_env, json, repo};

/// `--to <client>` reaches every session of that agent client in the repo, and
/// no other client's — the hand-off between providers (Claude → OpenCode).
#[test]
fn session_start_lists_an_issue_routed_to_the_agent_client() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let input = format!(r#"{{"cwd":{}}}"#, json(&d));
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "review the key order",
            "--files",
            "src/a.rs",
            "--to",
            "OpenCode",
        ],
        "",
    );
    assert!(ok, "{err}");
    // the receipt hands the sender a line to paste to the receiver
    let id = out.split_whitespace().next().unwrap();
    // an agent client also gets the headless command that starts it on the row
    let task = format!(
        "fael claim {id}, do what fael find {id} says, then fael close {id} '<what you did, how>'"
    );
    assert!(
        out.contains(&format!(
            "start it: env -u CLAUDE_CODE_SESSION_ID -u CODEX_THREAD_ID opencode run \"{task}\""
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!("to opencode: tell them `fael find {id}`")),
        "{out}"
    );
    // OpenCode speaks the neutral protocol: its client name rides in the Event
    let neutral = format!(r#"{{"cwd":{},"client":"opencode"}}"#, json(&d));
    let oc = fael(&d, &["hook", "session-start"], &neutral).1;
    let cl = fael(&d, &["hook", "session-start", "--client", "claude"], &input).1;
    assert!(oc.contains("review the key order (to: opencode)"), "{oc}");
    assert!(!cl.contains("review the key order"), "{cl}");
}

#[test]
fn a_person_gets_no_launch_line() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let args = [
        "add",
        "issue",
        "check the rounding",
        "--files",
        "src/a.rs",
        "--to",
        "ploy",
    ];
    let (ok, out, err) = fael(&d, &args, "");
    assert!(ok, "{err}");
    assert!(out.contains("to ploy: tell them"), "{out}");
    assert!(!out.contains("start it:"), "{out}");
}

/// Work passes Codex -> Claude -> OpenCode on the real payload fixtures: each
/// reader sees the row addressed to it, never the one it superseded, and
/// stats count one hand-off per client pair.
#[test]
fn relay_codex_to_claude_to_opencode() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "seed", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    let (codex, claude) = ("codex-relay", "claude-relay");
    std::fs::write(d.join(format!("{claude}.jsonl")), "").unwrap();
    // (1) a Codex edit, then the hand-off row to claude
    let mut edit = codex_fixture("edit", &d, codex);
    edit["tool_input"]["command"] = "*** Update File: src/a.rs".into();
    assert!(
        fael(
            &d,
            &["hook", "edit", "--client", "codex"],
            &edit.to_string()
        )
        .0
    );
    let first = "relay stopped at the parser";
    let (ok, out, err) = fael_env(
        &d,
        &[
            "add",
            "note",
            first,
            "--files",
            "src/a.rs",
            "--key",
            "relay:handoff",
            "--to",
            "claude",
        ],
        "",
        &[("CODEX_THREAD_ID", codex)],
    );
    assert!(ok, "{err}");
    let id1 = out.split_whitespace().next().unwrap().to_string();
    // (2) Claude's session-start holds it; a Claude edit; the next hand-off supersedes it
    let start = claude_fixture("start", &d, claude).to_string();
    let (ok, out, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &start);
    assert!(ok && out.contains(first), "{out}{err}");
    let mut edit = claude_fixture("edit", &d, claude);
    edit["tool_input"]["file_path"] = serde_json::json!(d.join("src/a.rs"));
    assert!(
        fael(
            &d,
            &["hook", "edit", "--client", "claude"],
            &edit.to_string()
        )
        .0
    );
    let second = "relay moved on to the printer";
    let (ok, _, err) = fael_env(
        &d,
        &[
            "add",
            "note",
            second,
            "--files",
            "src/a.rs",
            "--key",
            "relay:handoff",
            "--to",
            "opencode",
            "--supersedes",
            &id1,
        ],
        "",
        &[("CLAUDE_CODE_SESSION_ID", claude)],
    );
    assert!(ok, "{err}");
    // (3) OpenCode's neutral Event holds the second row, not the first
    let neutral = serde_json::json!({"cwd": d, "client": "opencode", "session": "oc-relay"});
    let (ok, oc, err) = fael(&d, &["hook", "session-start"], &neutral.to_string());
    assert!(
        ok && oc.contains(second) && !oc.contains(first),
        "{oc}{err}"
    );
    // (4) one hand-off per client pair
    let (ok, out, err) = fael(&d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["value"]["handoffs_by_client"]["codex→claude"], 1, "{out}");
    assert_eq!(
        v["value"]["handoffs_by_client"]["claude→opencode"], 1,
        "{out}"
    );
}
