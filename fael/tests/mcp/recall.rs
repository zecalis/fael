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
    assert!(!first.contains("closed issue on these files"), "{first}");
    let old = first.split_whitespace().nth(1).unwrap().to_string();
    let closed = mcp_tool(
        &wt,
        "close",
        &[serde_json::json!({"id": old, "text": "fixed"})],
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
    assert!(again.contains("closed issue on these files"), "{again}");
    // ids print at their shortest unique prefix
    let short = again.split("--supersedes ").nth(1).unwrap();
    let short = short.split(|c: char| !c.is_alphanumeric()).next().unwrap();
    assert!(old.starts_with(short) && short.len() >= 8, "{again}");
}
