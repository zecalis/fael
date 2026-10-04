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
    let (ok, out, _) = fael(&d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok && out.contains("parser half done"), "{out}");
    let (ok, _, _) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok);
    let usage = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = usage
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let read = lines.iter().find(|v| v["event"] == "read").unwrap();
    let kinds: Vec<&str> = read["said"]
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
    let pull: serde_json::Value = serde_json::from_str(usage.lines().last().unwrap()).unwrap();
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

/// A push in session s1 on `src/a.rs`.
fn push(d: &Path, event: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, _) = fael(d, &["hook", event, "--client", "claude"], &input);
    assert!(ok, "{out}");
    out
}

/// The count line's key is the pushed files; the call it prints may be those
/// files or their directory — both must earn it, and only after the pull.
#[test]
fn a_count_line_earns_on_the_call_it_printed() {
    // `+N more about this file — fael find --files src/a.rs`
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    for i in 0..7 {
        add(
            &d,
            &[
                "add",
                "decision",
                &format!("decision {i}"),
                "--files",
                "src/a.rs",
            ],
        );
    }
    let out = push(&d, "read");
    assert!(
        out.contains("more about this file — fael find --files src/a.rs"),
        "{out}"
    );
    assert_eq!(yield_of(&d, "count"), (1, 0));
    let env = [("FAEL_SESSION", "s1")];
    let (ok, _, err) = fael_env(&d, &["find", "--files", "src/a.rs"], "", &env);
    assert!(ok, "{err}");
    assert_eq!(yield_of(&d, "count"), (1, 1));
    // `+1 more in src/ — fael find --files src/`: a directory over the file
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    add(&d, &["add", "decision", "on a", "--files", "src/a.rs"]);
    add(&d, &["add", "decision", "neighbour", "--files", "src/b.rs"]);
    let out = push(&d, "edit");
    assert!(
        out.contains("+1 more in src/ — fael find --files src/"),
        "{out}"
    );
    assert_eq!(yield_of(&d, "count"), (1, 0));
    let (ok, _, err) = fael_env(&d, &["find", "--files", "src/"], "", &env);
    assert!(ok, "{err}");
    assert_eq!(yield_of(&d, "count"), (1, 1));
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
        serde_json::from_str::<serde_json::Value>(usage.lines().last().unwrap()).unwrap()
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

/// An issue on `src/a.rs` per key (filed on this branch, so session start puts
/// its key in the Focus) and a decision sibling on another file with that key
/// (tier 2 — kinds differ, or self-heal would supersede one). A tight budget
/// cuts the siblings, so the count names the key call. Returns the read push.
fn keyed(d: &Path, keys: &[&str]) -> String {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::create_dir_all(d.join("lib")).unwrap();
    for (i, k) in keys.iter().enumerate() {
        let lib = format!("lib/z{i}.rs");
        std::fs::write(d.join(&lib), "// z\n").unwrap();
        add(
            d,
            &[
                "add",
                "issue",
                &format!("on a {i}"),
                "--files",
                "src/a.rs",
                "--key",
                k,
            ],
        );
        let long = format!("sibling {i} with enough words to blow a tight push budget wide open");
        add(d, &["add", "decision", &long, "--files", &lib, "--key", k]);
    }
    let cfg = d.join(".fael/config.toml");
    let body = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(&cfg, body + "[budget]\npush_tokens = 1\n").unwrap();
    let input = format!(r#"{{"cwd":{},"session_id":"s1"}}"#, json(d));
    let (ok, _, err) = fael(d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err}");
    push(d, "read")
}

/// The key forms of the count line: `+N more with #<key> — fael find --key
/// <key>` earns on that key, never on a file pull; `+N more under N keys: …
/// — fael find --key <key>` earns on any key pull, while the file line beside
/// it does not.
#[test]
fn a_count_line_earns_on_the_key_it_names() {
    let env = [("FAEL_SESSION", "s1")];
    let find = |d: &Path, args: &[&str]| {
        let (ok, _, err) = fael_env(d, args, "", &env);
        assert!(ok, "{err}");
    };
    let d = repo();
    let out = keyed(&d, &["auth:session"]);
    assert!(
        out.contains("+1 more with #auth:session — fael find --key auth:session"),
        "{out}"
    );
    find(&d, &["find", "--files", "lib/z0.rs"]);
    assert_eq!(
        yield_of(&d, "count"),
        (1, 0),
        "a file pull is not the key call"
    );
    find(&d, &["find", "--key", "auth:session"]);
    assert_eq!(yield_of(&d, "count"), (1, 1));
    let d = repo();
    let out = keyed(&d, &["auth:session", "db:migrate"]);
    assert!(
        out.contains("more about this file — fael find --files src/a.rs"),
        "{out}"
    );
    assert!(out.contains("+2 more under 2 keys:"), "{out}");
    assert_eq!(yield_of(&d, "count"), (2, 0));
    find(&d, &["find", "--key", "auth:session"]);
    assert_eq!(
        yield_of(&d, "count"),
        (2, 1),
        "the keys line, not the file line"
    );
}
