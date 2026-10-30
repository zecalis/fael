//! MCP `find` answers like the CLI: filters, id shapes, text search.

use super::{main_and_worktree, mcp_tool, mcp_tool_env, texts};

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

/// PLAN-fael-id-refs chunk-1 + fael-secretary c2: MCP `find {"id"}` answers
/// exactly like the CLI — a real id pulls the body, a missing id-shaped query
/// rejects (naming the rows that only mention it). ULID-as-text auto-routes
/// (F-IDENT): the same string under `text` is the same id lookup — a typed
/// miss, never a silent text search and never a `find-misses.jsonl`
/// vocabulary miss. (`--text` on the CLI stays the literal-search hatch.)
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
    // the same string as `text` → the same id lookup (ULID-as-text
    // auto-routes, F-IDENT): a typed miss naming the mentioner, never a
    // silent text search — the citer is still named in the same call.
    let (is_err, body) = find(serde_json::json!({"text": fake}));
    assert!(
        is_err
            && body.contains("rejected: no row with id")
            && body.contains("copy the id from fael find")
            && body.contains("mentioned (not owned) by:"),
        "{body}"
    );
}

/// The opencode round-trip (2026-10-22): MCP `find {"id"}` for a row that
/// lives in another repo on this machine reads it there instead of
/// rejecting — ULIDs never collide, so one hit is the row. Writes never
/// follow; only this read does.
#[test]
fn find_id_follows_a_row_in_another_repo() {
    let (_, wt) = main_and_worktree();
    // a second, independent repo: its own journal, not this clone's
    let other = std::env::temp_dir().join(format!("fael-mcp-elsewhere-{}", fael_core::ulid()));
    std::fs::create_dir_all(other.join("src")).unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&other)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Elsewhere"]);
    git(&["config", "user.email", "elsewhere@example.com"]);
    std::fs::create_dir_all(other.join(".fael")).unwrap();
    std::fs::write(other.join("src/a.rs"), "// before\n").unwrap();
    let other = other.canonicalize().unwrap();
    // file the row there, through this server so `cwd` routing is exercised too
    let added = mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "note", "text": "row over there", "files": ["src/a.rs"], "cwd": other}),
        ],
    );
    let id = added[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string();
    // no `cwd`: the row is not here, but exactly one repo holds it
    let r = mcp_tool(&wt, "find", &[serde_json::json!({"id": id})]);
    assert_eq!(r[0]["result"]["isError"], false, "{r:?}");
    let body = r[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(body.contains("row over there"), "{body}");
    assert!(body.contains(&other.display().to_string()), "{body}");
    // A later session must see the owner's derived lifecycle state even
    // when it finds the row without knowing its repo first.
    std::fs::write(other.join("src/a.rs"), "// after\n").unwrap();
    let reads = mcp_tool(
        &wt,
        "find",
        &[
            serde_json::json!({"id": id, "cwd": other}),
            serde_json::json!({"id": id}),
        ],
    );
    for read in &reads {
        assert_eq!(read["result"]["isError"], false, "{read}");
        let text = read["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("(dormant)"), "{text}");
    }
    // a token no repo holds stays a miss, now with the MCP routing beside
    // the CLI one
    let r = mcp_tool(&wt, "find", &[serde_json::json!({"id": "01M4ZZZZZZZZ"})]);
    let miss = r[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        r[0]["result"]["isError"] == true
            && miss.contains("rejected: no row with id")
            && !miss.contains("MCP:"),
        "{miss}"
    );
}

/// f1: MCP `find {"recall": "wide"}` unions the exact file hits, the rows
/// sharing a key with one, and the text match off any file — in that order,
/// never the unrelated row; without it only today's exact hit.
#[test]
fn find_recall_wide_unions_key_sharing_and_unfiled_text() {
    let (_, wt) = main_and_worktree();
    for f in ["src/x.rs", "src/y.rs", "src/z.rs", "src/w.rs"] {
        std::fs::write(wt.join(f), "// x\n").unwrap();
    }
    // x and y share a key: past the 1 s same-burst window y would supersede x
    // and hide it — pin the window open so the setup never depends on the clock
    let added = mcp_tool_env(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "note", "text": "sunset deploy on x", "files": ["src/x.rs"], "key": "k"}),
            serde_json::json!({"kind": "note", "text": "unrelated bookkeeping", "files": ["src/y.rs"], "key": "k"}),
            serde_json::json!({"kind": "note", "text": "sunset deploy on z", "files": ["src/z.rs"]}),
            serde_json::json!({"kind": "note", "text": "unrelated other", "files": ["src/w.rs"]}),
        ],
        &[("FAEL_BURST_MS", "3600000")],
    );
    assert!(
        added.iter().all(|v| v["result"]["isError"] == false),
        "{added:?}"
    );
    let find = |args: serde_json::Value| {
        let r = mcp_tool(&wt, "find", &[args]);
        assert_eq!(r[0]["result"]["isError"], false, "{r:?}");
        r[0]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    // default: only today's exact hit
    let narrow = find(serde_json::json!({"files": ["src/x.rs"], "text": "sunset deploy"}));
    assert!(narrow.contains("sunset deploy on x"), "{narrow}");
    assert!(
        !narrow.contains("bookkeeping")
            && !narrow.contains("sunset deploy on z")
            && !narrow.contains("unrelated other"),
        "{narrow}"
    );
    // wide: exact, key-sharing, unfiled text — in that order, never D
    let wide =
        find(serde_json::json!({"files": ["src/x.rs"], "text": "sunset deploy", "recall": "wide"}));
    let (a, b, c) = (
        wide.find("sunset deploy on x"),
        wide.find("unrelated bookkeeping"),
        wide.find("sunset deploy on z"),
    );
    assert!(a.is_some() && b.is_some() && c.is_some(), "{wide}");
    assert!(a.unwrap() < b.unwrap() && b.unwrap() < c.unwrap(), "{wide}");
    assert!(!wide.contains("unrelated other"), "{wide}");
}
