//! A hand-off between agents: `--to <client>` and the receipt line.

use super::{fael, json, repo};

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
        out.contains(&format!("start it: opencode run \"{task}\"")),
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
    assert!(oc.contains("1 to you"), "{oc}");
    assert!(!cl.contains("review the key order"), "{cl}");
    assert!(!cl.contains("to you"), "{cl}");
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
