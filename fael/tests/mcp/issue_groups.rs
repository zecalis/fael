//! PLAN-fael-agent-ergonomics chunk 4: MCP `find {"kind": "issue"}` answers
//! grouped like the CLI — no `groups: true` needed, no tip left to learn.

use super::{main_and_worktree, mcp_tool};

#[test]
fn find_kind_issue_groups_by_default() {
    let (_, wt) = main_and_worktree();
    std::fs::write(wt.join("src/b.rs"), "// b\n").unwrap();
    let added = mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "issue", "text": "ocr timeout", "files": ["src/a.rs", "src/b.rs"]}),
            serde_json::json!({"kind": "issue", "text": "scope leak", "files": ["src/b.rs"]}),
        ],
    );
    assert!(
        added.iter().all(|v| v["result"]["isError"] == false),
        "{added:?}"
    );
    let r = mcp_tool(&wt, "find", &[serde_json::json!({"kind": "issue"})]);
    assert!(r[0]["result"]["isError"] == false, "{r:?}");
    let body = r[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        body.contains("## group 1 · 2 rows · shared: src/b.rs"),
        "{body}"
    );
    assert!(!body.contains("fix together"), "{body}");
}

/// Only-closed issues answer in the same call: the closed rows all=true would
/// match, marked, under the no-open-row header — additive, no field renamed.
#[test]
fn only_closed_issue_list_shows_marked_rows() {
    let (_, wt) = main_and_worktree();
    let added = mcp_tool(
        &wt,
        "add",
        &[serde_json::json!({"kind": "issue", "text": "ocr timeout", "files": ["src/a.rs"]})],
    );
    assert!(
        added.iter().all(|v| v["result"]["isError"] == false),
        "{added:?}"
    );
    let id = added[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string();
    let closed = mcp_tool(
        &wt,
        "close",
        &[serde_json::json!({"id": id, "text": "fixed"})],
    );
    assert!(
        closed.iter().all(|v| v["result"]["isError"] == false),
        "{closed:?}"
    );
    let r = mcp_tool(&wt, "find", &[serde_json::json!({"kind": "issue"})]);
    assert!(r[0]["result"]["isError"] == false, "{r:?}");
    let body = r[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        body.starts_with("no open row matches — 1 closed or superseded:")
            && body.contains("(closed)")
            && body.contains("ocr timeout"),
        "{body}"
    );
}

/// An empty grouped answer says why like the flat one: closed rows need all=true.
#[test]
fn an_empty_issue_list_says_all_adds_closed() {
    let (_, wt) = main_and_worktree();
    let r = mcp_tool(&wt, "find", &[serde_json::json!({"kind": "issue"})]);
    let body = r[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        body.starts_with("no rows match kind=issue (open rows only; --all adds closed)"),
        "{body}"
    );
}
