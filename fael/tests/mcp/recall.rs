//! Closed-issue recall (PLAN-fael-context-loop chunk 5), MCP side: the `add`
//! answer names a closed issue on the same file, and stays silent without one.

use super::{main_and_worktree, mcp_tool};

fn text(r: &serde_json::Value) -> String {
    assert!(r["result"]["isError"] == false, "{r:?}");
    r["result"]["content"][0]["text"].as_str().unwrap().into()
}

#[test]
fn add_issue_names_the_closed_issue_on_its_file() {
    let (_, wt) = main_and_worktree();
    let first = mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "issue", "text": "heic breaks in chrome", "files": ["src/a.rs"]}),
        ],
    );
    let first = text(&first[0]);
    assert!(!first.contains("closed earlier on these files"), "{first}");
    let old = first.split_whitespace().nth(1).unwrap().to_string();
    let closed = mcp_tool(
        &wt,
        "close",
        &[
            serde_json::json!({"id": old, "text": "heic broke → pinned the decoder; guard `src/a_test.rs`"}),
        ],
    );
    text(&closed[0]);
    let again = mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "issue", "text": "chrome rejects heic again", "files": ["src/a.rs"]}),
        ],
    );
    let again = text(&again[0]);
    assert!(again.contains("closed earlier on these files"), "{again}");
    // ids print at their shortest unique prefix
    let short = again.split("--supersedes ").nth(1).unwrap();
    let short = short.split(|c: char| !c.is_alphanumeric()).next().unwrap();
    assert!(old.starts_with(short) && short.len() >= 8, "{again}");
}

#[test]
fn follows_silences_the_closed_line_and_then_files_a_child_issue() {
    let (_, wt) = main_and_worktree();
    let add = |v: serde_json::Value| text(&mcp_tool(&wt, "add", &[v])[0]);
    let first = add(
        serde_json::json!({"kind": "issue", "text": "heic breaks in chrome", "files": ["src/a.rs"]}),
    );
    let old = first.split_whitespace().nth(1).unwrap().to_string();
    text(
        &mcp_tool(
            &wt,
            "close",
            &[
                serde_json::json!({"id": old, "text": "heic broke → pinned the decoder; guard `src/a_test.rs`"}),
            ],
        )[0],
    );
    let next = add(
        serde_json::json!({"kind": "issue", "text": "chrome rejects heic again", "files": ["src/a.rs"], "follows": old}),
    );
    assert!(!next.contains("closed earlier on these files"), "{next}");
    // `then`: a note and an issue that follows it, one call
    let both = add(
        serde_json::json!({"kind": "note", "text": "8 queries measured at 11k docs", "files": ["src/a.rs"], "then": "measure with the real provider"}),
    );
    assert_eq!(both.matches("recorded").count(), 2, "{both}");
    assert!(both.contains("(follows "), "{both}");
}
