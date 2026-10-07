//! MCP `find` answers like the CLI: filters, id shapes, text search.

use super::{main_and_worktree, mcp_tool, texts};

/// Issue 01M3HMYS: MCP `find` gains the CLI's `by` and `all` filters.
#[test]
fn find_filters_by_writer_and_includes_closed() {
    let (_, wt) = main_and_worktree();
    // a second file, so the two notes do not auto-supersede each other
    std::fs::write(wt.join("src/b.rs"), "// b\n").unwrap();
    mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "note", "text": "keeper row one", "files": ["src/a.rs"]}),
            serde_json::json!({"kind": "note", "text": "keeper row two", "files": ["src/b.rs"]}),
        ],
    );
    let raw = texts(&wt);
    let writer = raw
        .split("\"by\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let id = raw
        .lines()
        .find(|l| l.contains("keeper row two"))
        .unwrap()
        .split("\"id\":\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    mcp_tool(
        &wt,
        "close",
        &[serde_json::json!({"id": id, "text": "done"})],
    );
    let find = |args: serde_json::Value| {
        let r = mcp_tool(&wt, "find", &[args]);
        r[0]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // closed rows stay hidden without `all`, and show with it
    let open = find(serde_json::json!({"text": "keeper row"}));
    assert!(
        open.contains("keeper row one") && !open.contains("keeper row two"),
        "{open}"
    );
    let all = find(serde_json::json!({"text": "keeper row", "all": true}));
    assert!(all.contains("keeper row two"), "{all}");
    // `by` narrows to this writer; a bogus writer matches nothing
    let mine = find(serde_json::json!({"by": writer}));
    assert!(mine.contains("keeper row one"), "{mine}");
    let none = find(serde_json::json!({"by": "no-such-writer"}));
    // an empty find names the dead filter
    assert!(
        none.starts_with("no rows match by=no-such-writer"),
        "{none}"
    );
}

/// PLAN-fael-id-refs chunk-1: MCP `find {"id"}` answers exactly like the CLI —
/// a real id pulls the body, a missing id-shaped query rejects (naming the
/// rows that only mention it), and the same string under `text` is a literal
/// text search.
#[test]
fn find_id_shapes_match_cli() {
    let (_, wt) = main_and_worktree();
    std::fs::write(wt.join("src/b.rs"), "// b\n").unwrap();
    let added = mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "note", "text": "keeper row", "files": ["src/a.rs"]}),
            serde_json::json!({"kind": "note", "text": "citing row", "files": ["src/b.rs"]}),
        ],
    );
    assert!(
        added.iter().all(|v| v["result"]["isError"] == false),
        "{added:?}"
    );
    let id_of = |v: &serde_json::Value| {
        v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_string()
    };
    let (id, _citing) = (id_of(&added[0]), id_of(&added[1]));
    // the citing row mentions a phantom id: flip the keeper's last char, so
    // the 26-char token is Missing (only an exact id could own it)
    let mut fake = id.clone();
    fake.pop();
    fake.push(if id.ends_with('A') { 'B' } else { 'A' });
    assert!(fael_core::looks_like_id(&fake), "{fake}");
    std::fs::write(wt.join("src/c.rs"), "// c\n").unwrap();
    let cited = mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "note", "text": format!("see {fake} for context"),
            "files": ["src/c.rs"]}),
        ],
    );
    assert!(
        cited.iter().all(|v| v["result"]["isError"] == false),
        "{cited:?}"
    );

    let find = |args: serde_json::Value| {
        let r = mcp_tool(&wt, "find", &[args]);
        (
            r[0]["result"]["isError"] == true,
            r[0]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_string(),
        )
    };
    // real id → the body, like `fael find <id>` (nothing cites it: the phantom
    // token is longer than the keeper's id prefix, so no mentioned-by line)
    let (is_err, body) = find(serde_json::json!({"id": id}));
    assert!(!is_err && body.contains("keeper row"), "{body}");
    assert!(!body.contains("mentioned by:"), "{body}");
    // missing id → the CLI's reject, naming the mentioner by its short id
    let (is_err, body) = find(serde_json::json!({"id": fake}));
    assert!(
        is_err
            && body.contains("rejected: no row with id")
            && body.contains("copy the id from fael find")
            && body.contains("mentioned (not owned) by:"),
        "{body}"
    );
    // the same string as `text` → the literal text search, no reject
    let (is_err, body) = find(serde_json::json!({"text": fake}));
    assert!(!is_err && body.contains("for context"), "{body}");
}
