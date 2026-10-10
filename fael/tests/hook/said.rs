//! PLAN-fael-say-gate chunk 3: a push's usage line says what it said per
//! kind, a note in context at an edit lands under `in_context_notes`, a
//! successful find writes its outcome line — and `fael stats` joins them
//! into yield per kind.

use super::{fael, fael_env, json, repo};
use std::path::Path;

#[test]
fn said_notes_and_finds_join_into_yield() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    for (kind, text) in [
        ("decision", "keep the parser pure"),
        ("note", "parser half done"),
    ] {
        let (ok, _, err) = fael(&d, &["add", kind, text, "--files", "src/a.rs"], "");
        assert!(ok, "{err}");
    }
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&d.join("src/a.rs"))
    );
    // the first edit says the rows, the second finds them in context
    let (ok, out, _) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok && out.contains("parser half done"), "{out}");
    let (ok, _, _) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok);
    let usage = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = usage
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let edit = lines.iter().find(|v| v["event"] == "edit").unwrap();
    let kinds: Vec<&str> = edit["said"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["row", "row"], "{usage}");
    let ctx = lines.iter().find(|v| v["event"] == "in-context").unwrap();
    assert_eq!(ctx["in_context"].as_array().unwrap().len(), 1, "{usage}");
    assert_eq!(
        ctx["in_context_notes"].as_array().unwrap().len(),
        1,
        "{usage}"
    );
    // a find in the same session writes a pull line, never free text
    let env = [("FAEL_SESSION", "s1")];
    let (ok, _, err) = fael_env(&d, &["find", "parser", "--files", "src/a.rs"], "", &env);
    assert!(ok, "{err}");
    let usage = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap();
    let pull: serde_json::Value = serde_json::from_str(
        usage
            .lines()
            .rev()
            .find(|l| !l.contains("\"event\":\"call\""))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(pull["event"], "find", "{usage}");
    assert_eq!(pull["session"], "s1", "{usage}");
    assert_eq!(pull["found"].as_array().unwrap().len(), 2, "{usage}");
    assert_eq!(
        pull["q"],
        serde_json::json!({"files": ["src/a.rs"]}),
        "{usage}"
    );
    assert!(!usage.contains("\"parser\""), "free text stored: {usage}");
    let (ok, out, _) = fael(&d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(ok, "{out}");
    assert_eq!(
        v["said"]["row"],
        serde_json::json!({"said": 1, "earned": 1}),
        "{out}"
    );
    assert_eq!(
        v["said"]["note"],
        serde_json::json!({"said": 1, "earned": 1}),
        "{out}"
    );
    // the pull line is no injection: read + edit only
    assert_eq!(v["events"], 2, "{out}");
}

/// `fael stats --json` → `said.<kind>` as (said, earned).
fn yield_of(d: &Path, kind: &str) -> (u64, u64) {
    let (ok, out, _) = fael(d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(ok, "{out}");
    let y = &v["said"][kind];
    (y["said"].as_u64().unwrap(), y["earned"].as_u64().unwrap())
}

fn add(d: &Path, args: &[&str]) {
    let (ok, _, err) = fael(d, args, "");
    assert!(ok, "{err}");
}

/// A prompt pointer is earned by an MCP find on its key; the MCP pull line
/// carries the session and the query, and an id lookup records the shown id.
#[test]
fn a_pointer_earns_on_an_mcp_find_by_its_key() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let key = "vela:credit-ledger";
    add(
        &d,
        &[
            "add",
            "decision",
            "append-only",
            "--files",
            "src/a.rs",
            "--key",
            key,
        ],
    );
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","prompt":"is there credit ledger code yet?"}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "prompt", "--client", "claude"], &input);
    assert!(ok && out.contains(key), "{out}");
    assert_eq!(yield_of(&d, "pointer"), (1, 0));
    let call = |args: serde_json::Value| {
        let rpc = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "find", "arguments": args}});
        let env = [("FAEL_SESSION", "s1")];
        let (ok, out, err) = fael_env(&d, &["mcp"], &format!("{rpc}\n"), &env);
        assert!(ok && out.contains("append-only"), "{out}{err}");
        let usage = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap();
        serde_json::from_str::<serde_json::Value>(
            usage
                .lines()
                .rev()
                .find(|l| !l.contains("\"event\":\"call\""))
                .unwrap(),
        )
        .unwrap()
    };
    let pull = call(serde_json::json!({"key": key}));
    assert_eq!(
        (&pull["event"], &pull["session"], &pull["q"]),
        (
            &"mcp-find".into(),
            &"s1".into(),
            &serde_json::json!({"key": key})
        ),
        "{pull}"
    );
    assert_eq!(yield_of(&d, "pointer"), (1, 1));
    // an id lookup: q.id is the row shown, never the typed text
    let id = pull["found"][0].as_str().unwrap().to_string();
    let pull = call(serde_json::json!({"id": &id[..10]}));
    assert_eq!(pull["q"], serde_json::json!({"id": id}), "{pull}");
}

/// PLAN-fael-agent-ergonomics chunk 5: a `git commit` naming an open issue
/// says its ready close once per session — and the line is measured, so the
/// keep/cut bar reads it from `fael stats`.
#[test]
fn commit_citing_an_open_issue_says_its_close_once() {
    let d = repo();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "leaks a handle",
            "--files",
            "src/b.rs",
            "--json",
        ],
        "",
    );
    assert!(ok, "{err}");
    let id = out
        .lines()
        .find_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .and_then(|v| v["id"].as_str().map(String::from))
        .expect(&out);
    let commit = |msg: &str| commit_ctx(&d, msg);
    let ctx = commit(&format!("fix {id}")).expect("the first commit is said");
    assert!(ctx.contains("cited in a commit"), "{ctx}");
    // one row in the log: abbrev prints the 8-char prefix
    let short = &id[..8];
    assert!(ctx.contains(&format!("`(fael:{short})`")), "{ctx}");
    assert!(ctx.contains(&format!("fael close {short} ")), "{ctx}");
    assert_eq!(yield_of(&d, "cited"), (1, 0));
    assert!(
        commit(&format!("fix {id} again")).is_none(),
        "once per id per session"
    );
    assert_eq!(yield_of(&d, "cited"), (1, 0), "no second said");
}

/// A `git commit -m '<msg>'` through the search hook: the reply's context, if
/// any — silence prints nothing at all.
fn commit_ctx(d: &Path, msg: &str) -> Option<String> {
    let payload = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Bash","tool_input":{{"command":{}}},"tool_response":{{}}}}"#,
        json(d),
        serde_json::to_string(&format!("git commit -m '{msg}'")).unwrap(),
    );
    let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &payload);
    assert!(ok, "{err}");
    match out.trim().is_empty() {
        true => None,
        false => serde_json::from_str::<serde_json::Value>(&out).expect(&out)["hookSpecificOutput"]
            ["additionalContext"]
            .as_str()
            .map(String::from),
    }
}

/// Two open issues filed in one millisecond share their first 8 chars (vela
/// `01M4G1N1`): the Cited line's `(fael:<prefix>)` must still name one row,
/// or the resolver rejects the citation it taught as ambiguous.
#[test]
fn cited_prefix_stays_unique_when_rows_share_eight_chars() {
    let d = repo();
    let (a, b) = ("01M4G1N16R2X0000000000000A", "01M4G1N199XP0000000000000B");
    let row = |id: &str| {
        format!(
            r#"{{"v":1,"id":"{id}","ts":"2026-10-01T00:00:00.000Z","by":"fixture","kind":"issue","text":"leaks a handle","files":["src/b.rs"]}}"#
        )
    };
    std::fs::create_dir_all(d.join(".fael/log/fixture")).unwrap();
    std::fs::write(
        d.join(".fael/log/fixture/2026-10.jsonl"),
        format!("{}\n{}\n", row(a), row(b)),
    )
    .unwrap();
    let ctx = commit_ctx(&d, &format!("fix {a}")).expect("the commit is said");
    let short = ctx
        .split("`(fael:")
        .nth(1)
        .and_then(|t| t.split(')').next())
        .expect(&ctx);
    assert!(short.len() > 8, "{ctx}");
    assert!(a.starts_with(short) && !b.starts_with(short), "{ctx}");
    assert!(ctx.contains(&format!("fael close {short} ")), "{ctx}");
}
