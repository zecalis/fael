//! `fael mcp` picks the repo per call — the server runs in the session's cwd,
//! an agent may be working in another worktree.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(d)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// `main/` with a commit, and `wt/` checked out from it as a git worktree.
fn main_and_worktree() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("fael-mcp-{}", fael_core::ulid()));
    let (main, wt) = (base.join("main"), base.join("wt"));
    std::fs::create_dir_all(main.join("src")).unwrap();
    git(&main, &["init", "-q"]);
    git(&main, &["config", "user.name", "Mcp Test"]);
    git(&main, &["config", "user.email", "mcp@example.com"]);
    std::fs::write(main.join("src/a.rs"), "// a\n").unwrap();
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(main.join(".fael")).unwrap();
    std::fs::write(main.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "init"]);
    git(&main, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    (main.canonicalize().unwrap(), wt.canonicalize().unwrap())
}

fn mcp(dir: &Path, calls: &[serde_json::Value]) -> Vec<serde_json::Value> {
    mcp_tool(dir, "add", calls)
}

fn mcp_tool(dir: &Path, tool: &str, calls: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", dir.join("../state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let lines: Vec<String> = calls
        .iter()
        .enumerate()
        .map(|(i, a)| {
            serde_json::json!({"jsonrpc": "2.0", "id": i, "method": "tools/call",
                "params": {"name": tool, "arguments": a}})
            .to_string()
        })
        .collect();
    let mut stdin = c.stdin.take().unwrap();
    stdin
        .write_all((lines.join("\n") + "\n").as_bytes())
        .unwrap();
    drop(stdin);
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    out.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// Every row text in `<root>/.fael/log`.
fn texts(root: &Path) -> String {
    let mut all = String::new();
    let log = root.join(".fael/log");
    for dir in std::fs::read_dir(&log).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            all += &std::fs::read_to_string(f.path()).unwrap_or_default();
        }
    }
    all
}

#[test]
fn add_lands_in_the_worktree_the_call_names() {
    let (main, wt) = main_and_worktree();
    let r = mcp(
        &main,
        &[
            serde_json::json!({"kind": "note", "text": "by cwd", "files": ["src/a.rs"], "cwd": wt}),
            serde_json::json!({"kind": "note", "text": "by abs path", "files": [wt.join("src/a.rs")]}),
            serde_json::json!({"kind": "note", "text": "plain", "files": ["src/a.rs"]}),
        ],
    );
    assert!(r.iter().all(|v| v["result"]["isError"] == false), "{r:?}");
    let (m, w) = (texts(&main), texts(&wt));
    assert!(w.contains("by cwd") && w.contains("by abs path"), "{w}");
    assert!(!m.contains("by cwd") && !m.contains("by abs path"), "{m}");
    assert!(m.contains("plain") && !w.contains("plain"), "{m}");
    // the absolute path is stored repo-relative, like any other
    assert!(w.contains(r#""src/a.rs""#), "{w}");
}

/// Chunk 6b over MCP: `rows: [...]` with one bad row — it reports alone
/// (`rejected: row 1:`), the rest save, the call is an error.
#[test]
fn add_rows_batch_partial() {
    let (_, wt) = main_and_worktree();
    let r = mcp(
        &wt,
        &[serde_json::json!({"rows": [
            {"kind": "note", "text": "first mcp batch row", "files": ["src/a.rs"]},
            {"kind": "nope", "text": "bad kind row", "files": ["src/a.rs"]},
            {"kind": "issue", "text": "third mcp batch row", "files": ["src/a.rs"]},
        ]})],
    );
    assert_eq!(r.len(), 1);
    let body = r[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r[0]["result"]["isError"] == true, "{body}");
    assert!(body.contains("recorded"), "{body}");
    assert!(body.contains("rejected: row 1:"), "{body}");
    let log = texts(&wt);
    assert!(
        log.contains("first mcp batch row") && log.contains("third mcp batch row"),
        "{log}"
    );
    assert!(!log.contains("bad kind row"), "{log}");
}

/// A routed row over MCP gets the CLI's paste line, under the id line the
/// callers parse.
#[test]
fn add_to_a_client_prints_the_paste_line() {
    let (_, wt) = main_and_worktree();
    let r = mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "issue", "text": "review this", "files": ["src/a.rs"], "to": "opencode"}),
        ],
    );
    let body = r[0]["result"]["content"][0]["text"].as_str().unwrap();
    let mut lines = body.lines();
    let id = lines.next().unwrap()["recorded ".len()..].to_string();
    assert_eq!(
        lines.next(),
        Some(format!("to opencode: tell them `fael find {id}`").as_str()),
        "{body}"
    );
    let r = mcp(
        &wt,
        &[
            serde_json::json!({"rows": [{"kind": "issue", "text": "batch review", "files": ["src/a.rs"], "to": "codex"}]}),
        ],
    );
    let body = r[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(body.contains("to codex: tell them `fael find "), "{body}");
}

/// PLAN-fael-id-refs chunk-2: MCP `add`/`close` carry the same phantom info
/// line as the CLI — the row is still recorded, never rejected.
#[test]
fn write_phantom_info_line_matches_cli() {
    let (_, wt) = main_and_worktree();
    std::fs::write(wt.join("src/b.rs"), "// b\n").unwrap();
    let body = |r: &[serde_json::Value]| {
        r[0]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let r = mcp_tool(
        &wt,
        "add",
        &[
            serde_json::json!({"kind": "note", "text": "see 01DEFACED01 for context", "files": ["src/b.rs"]}),
        ],
    );
    let added = body(&r);
    assert!(
        added.contains("recorded ")
            && added.contains("no row with id 01DEFACED01")
            && added.contains("copy ids from fael find"),
        "{added}"
    );
    let r = mcp_tool(
        &wt,
        "add",
        &[serde_json::json!({"kind": "issue", "text": "broken thing", "files": ["src/b.rs"]})],
    );
    let id = body(&r).lines().next().unwrap()["recorded ".len()..].to_string();
    let r = mcp_tool(
        &wt,
        "close",
        &[serde_json::json!({"id": id, "text": "fixed, see 01DEFACED01"})],
    );
    let closed = body(&r);
    assert!(
        closed.contains("recorded ") && closed.contains("no row with id 01DEFACED01"),
        "{closed}"
    );
}
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
    // an empty find counts each part alone, so the dead filter shows
    assert!(
        none.starts_with("no rows match") && none.contains("by=no-such-writer ×0"),
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
    // real id → the body, like `fael find <id>`
    let (is_err, body) = find(serde_json::json!({"id": id}));
    assert!(!is_err && body.contains("keeper row"), "{body}");
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

/// MCP `close` takes `ids: [...]` like the CLI's `close a b "why"`: a bad id
/// reports alone and turns the call into an error, the rest still close.
#[test]
fn close_ids_closes_the_rest_and_names_the_bad_one() {
    let (_, wt) = main_and_worktree();
    std::fs::write(wt.join("src/b.rs"), "// b\n").unwrap();
    let text = |r: &[serde_json::Value]| {
        r[0]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let file = |t: &str| {
        let r = mcp_tool(
            &wt,
            "add",
            &[serde_json::json!({"kind": "issue", "text": t, "files": ["src/b.rs"]})],
        );
        text(&r).lines().next().unwrap()["recorded ".len()..].to_string()
    };
    let (a, b) = (file("first broken thing"), file("second odd thing"));
    let r = mcp_tool(
        &wt,
        "close",
        &[serde_json::json!({"ids": [a, "01NOSUCHROW", b], "text": "fixed"})],
    );
    assert_eq!(r[0]["result"]["isError"], true);
    let out = text(&r);
    // each close files its own row: two recorded, the bad id named
    assert_eq!(out.matches("recorded ").count(), 2, "{out}");
    assert!(out.contains("rejected: 01NOSUCHROW"), "{out}");
    // both really closed: closing one again is refused
    let again = mcp_tool(
        &wt,
        "close",
        &[serde_json::json!({"id": a, "text": "again"})],
    );
    assert!(text(&again).contains("closed"), "{}", text(&again));
}
