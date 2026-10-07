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
