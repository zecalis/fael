//! PLAN-fael-context-loop chunk 4: a decision in the agent's context at edits
//! of its file in ten sessions is asked once ever, at an edit after the next
//! session start, whether it should be a test or a lint/check; nine is silent.

use super::{fael, fael_at, git, json, repo, state};
use std::path::Path;

const ASK: &str = "should it be a test or a lint/check";
const RULE: &str = "a keeps one door";

/// A hook event on `<dir>/src/a.rs`, with usage in `st` (one state dir per
/// machine, whichever worktree the agent is in).
fn hook_at(st: &Path, d: &Path, event: &str, session: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, err) = fael_at(st, d, &["hook", event, "--client", "claude"], &input);
    assert!(ok, "{err}");
    out
}

fn hook(d: &Path, event: &str, session: &str) -> String {
    hook_at(&state(d), d, event, session)
}

/// One decision on `src/a.rs`, its short id.
fn decision(d: &Path) -> String {
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let (ok, out, err) = fael(d, &["add", "decision", RULE, "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// Sessions `from..to` each edit `src/a.rs` twice: the first edit says the
/// row, the second finds it in context (the usage `in-context` event).
fn sessions_with_row_in_context(d: &Path, from: usize, to: usize) {
    for s in from..to {
        hook(d, "edit", &format!("s{s}"));
        hook(d, "edit", &format!("s{s}"));
    }
}

fn yield_of(d: &Path) -> (u64, u64) {
    let (_, out, _) = fael(d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let y = &v["said"]["promote"];
    (y["said"].as_u64().unwrap(), y["earned"].as_u64().unwrap())
}

#[test]
fn ten_sessions_ask_once_ever_and_a_close_earns_it() {
    let d = repo();
    let id = decision(&d);
    sessions_with_row_in_context(&d, 0, 9);
    hook(&d, "session-start", "n9");
    assert!(
        !hook(&d, "edit", "n9").contains(ASK),
        "nine sessions are silent"
    );
    sessions_with_row_in_context(&d, 9, 10);
    hook(&d, "session-start", "n10");
    // a read says the row, never the ask
    let read = hook(&d, "read", "n10");
    assert!(read.contains(RULE) && !read.contains(ASK), "{read}");
    let asked = hook(&d, "edit", "n10");
    assert!(asked.contains(ASK), "{asked}");
    assert!(asked.contains("fael close "), "{asked}");
    assert!(!hook(&d, "edit", "n10").contains(ASK), "once per session");
    hook(&d, "session-start", "n11");
    assert!(!hook(&d, "edit", "n11").contains(ASK), "once ever");
    assert_eq!(yield_of(&d), (1, 0));
    // a bump keeps the rule a row: not moved into a test or check
    let (ok, _, err) = fael(&d, &["bump", &id], "");
    assert!(ok, "{err}");
    assert_eq!(yield_of(&d), (1, 0), "a bump does not earn");
    let (ok, _, err) = fael(&d, &["close", &id, "moved to tests/a.rs"], "");
    assert!(ok, "{err}");
    assert_eq!(yield_of(&d), (1, 1));
}

/// The cache lives with the clone, not the worktree: a session start in the
/// main checkout fills it, an edit in another worktree asks.
#[test]
fn another_worktree_reads_the_same_cache() {
    let d = repo();
    // rows in the clone's journal, which every worktree shares
    std::fs::write(d.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
    decision(&d);
    sessions_with_row_in_context(&d, 0, 10);
    hook(&d, "session-start", "n10");
    let wt = d.with_file_name(format!("{}-wt", d.file_name().unwrap().to_string_lossy()));
    git(
        &d,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wt"],
    );
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join("src/a.rs"), "// v1\n").unwrap();
    let asked = hook_at(&state(&d), &wt, "edit", "w1");
    assert!(asked.contains(ASK), "{asked}");
}
