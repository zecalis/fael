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
