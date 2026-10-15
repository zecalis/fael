//! OpenCode adapter: V1 + V2 tool shapes record their files, and the
//! adapter replies what neutral does, byte for byte.

use super::{fael, repo, state};

const S: &str = "2025-12-01T00:00:00Z";

/// Every path the edit hook recorded, across sessions.
fn recorded(d: &std::path::Path) -> String {
    std::fs::read_dir(state(d).join("sessions"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect()
}

/// An edit records only once the journal holds a row (`note_edit` needs a
/// home) — the same `add` first every recording test does.
fn with_row(d: &std::path::Path) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        d,
        &["add", "decision", "shape probe", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
}

#[test]
fn v1_and_v2_tool_shapes_record_without_files() {
    let d = repo();
    with_row(&d);
    for f in ["src/b.rs", "src/c.rs"] {
        std::fs::write(d.join(f), "//\n").unwrap();
    }
    // V1 `apply_patch` carries the paths in its patch text, never as files
    let v1 = serde_json::json!({"cwd": d, "client": "opencode", "session": S,
        "tool": "apply_patch",
        "tool_input": {"patchText": "*** Begin Patch\n*** Add File: src/b.rs\n+//\n*** End Patch\n"}})
    .to_string();
    let (ok, out, err) = fael(&d, &["hook", "edit"], &v1);
    assert!(ok, "{err} {out}");
    // V2 `patch` is the same shape under its own name
    let v2 = serde_json::json!({"cwd": d, "client": "opencode", "session": S,
        "tool": "patch",
        "tool_input": {"patchText": "*** Begin Patch\n*** Add File: src/c.rs\n+//\n*** End Patch\n"}})
    .to_string();
    let (ok, out, err) = fael(&d, &["hook", "edit"], &v2);
    assert!(ok, "{err} {out}");
    // V2 `write` names V1's `filePath` as `path`
    let v2path = serde_json::json!({"cwd": d, "client": "opencode", "session": S,
        "tool": "write", "tool_input": {"path": "src/c.rs"}})
    .to_string();
    let (ok, out, err) = fael(&d, &["hook", "edit"], &v2path);
    assert!(ok, "{err} {out}");
    let got = recorded(&d);
    assert!(got.contains("src/b.rs"), "{got}");
    assert!(got.contains("src/c.rs"), "{got}");
}

#[test]
fn routed_tool_never_overrides_sent_files() {
    let d = repo();
    with_row(&d);
    std::fs::write(d.join("src/sent.rs"), "//\n").unwrap();
    let body = serde_json::json!({"cwd": d, "client": "opencode", "session": S,
        "files": ["src/sent.rs"],
        "tool": "write", "tool_input": {"path": "src/other.rs"}})
    .to_string();
    let (ok, out, err) = fael(&d, &["hook", "edit"], &body);
    assert!(ok, "{err} {out}");
    let got = recorded(&d);
    assert!(got.contains("src/sent.rs"), "{got}");
    assert!(!got.contains("src/other.rs"), "{got}");
}

#[test]
fn adapter_matches_neutral_byte_for_byte() {
    use serde_json::json;
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "the adapter shapes opencode events",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    // the same event twice: once bare (neutral), once as the plugin sends
    // it (`client: "opencode"`) — separate sessions, so neither run's seen
    // list leaks into the other
    let cases = [
        ("session-start", json!({})),
        ("read", json!({"files": ["src/a.rs"]})),
        ("edit", json!({"files": ["src/a.rs"]})),
        ("search", json!({"files": ["src/a.rs"]})),
        (
            "search",
            json!({"tool": "shell", "tool_input": {"command": "true"}, "tool_response": {}}),
        ),
        (
            "stop",
            json!({"text": "wired the adapter", "reply": "wired the adapter"}),
        ),
        ("prompt", json!({"text": "check the adapter wiring"})),
    ];
    for (hook, mut base) in cases {
        let mut plain = base.clone();
        plain["cwd"] = json!(&d);
        plain["session"] = json!("s-neutral");
        let (ok, want, err) = fael(&d, &["hook", hook], &plain.to_string());
        assert!(ok, "{hook}: {err}");
        base["cwd"] = json!(&d);
        base["session"] = json!("s-opencode");
        base["client"] = json!("opencode");
        let (ok, got, err) = fael(&d, &["hook", hook], &base.to_string());
        assert!(ok, "{hook}: {err}");
        assert_eq!(got, want, "{hook} drifted from neutral");
    }
}

/// oc-prompt: the shape the V2 `prompt` admission hook forwards — the user's
/// text, nothing else — gets the one pointer line for an open key head, once
/// per session, and stays silent otherwise. The JS never matches keys; this
/// is the server side it relies on.
#[test]
fn opencode_prompt_points_at_named_key_once() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "append-only",
            "--files",
            "src/a.rs",
            "--key",
            "vela:credit-ledger",
        ],
        "",
    );
    assert!(ok, "{err}");
    let ask = |s: &str, p: &str| {
        let body = serde_json::json!({"cwd": d, "client": "opencode", "session": s, "text": p})
            .to_string();
        fael(&d, &["hook", "prompt"], &body)
    };
    let (ok, out, _) = ask(S, "is there credit ledger code yet?");
    // neutral prints the Reply JSON-escaped, like the claude test's shape
    assert!(
        ok && out.contains("vela:credit-ledger (1, via \\\"credit ledger\\\")"),
        "{out}"
    );
    // pointer only — the row itself never rides the prompt
    assert!(!out.contains("append-only"), "{out}");
    // once per session (neutral prints {"block":false}, never empty)
    let (ok, out, _) = ask(S, "credit ledger again");
    assert!(ok && !out.contains("context"), "{out}");
    // no head named = silent, no turn started by us (Reply has no prompt)
    let (ok, out, _) = ask("s2", "how does auth work?");
    assert!(ok && !out.contains("context"), "{out}");
}
