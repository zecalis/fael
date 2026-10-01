//! UserPromptSubmit: a prompt naming an open key gets one pointer line,
//! once per session; nothing named = nothing printed.

use super::{fael, json, repo};

#[test]
fn prompt_points_at_named_key_once() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let add = "add decision append-only --files src/a.rs --key vela:credit-ledger";
    let (ok, _, err) = fael(&d, &add.split(' ').collect::<Vec<_>>(), "");
    assert!(ok, "{err}");
    let ask = |s: &str, p: &str| {
        let input = format!(
            r#"{{"cwd":{},"session_id":"{s}","prompt":{}}}"#,
            json(&d),
            serde_json::to_string(p).unwrap()
        );
        fael(&d, &["hook", "prompt", "--client", "claude"], &input)
    };
    let (ok, out, _) = ask("s1", "is there credit code yet?");
    assert!(
        ok && out.contains("UserPromptSubmit") && out.contains("vela:credit-ledger (1)"),
        "{out}"
    );
    // pointer only — the row itself never rides the prompt
    assert!(!out.contains("append-only"), "{out}");
    // once per session
    let (ok, out, _) = ask("s1", "credit again");
    assert!(ok && out.is_empty(), "{out}");
    // a fresh session: no exact segment = silent, the exact one still points
    let (ok, out, _) = ask("s2", "credits ledge");
    assert!(ok && out.is_empty(), "{out}");
    let (ok, out, _) = ask("s2", "the ledger");
    assert!(ok && out.contains("vela:credit-ledger"), "{out}");
    // no session = no once-only list, so no hint
    let (ok, out, _) = ask("", "credit");
    assert!(ok && out.is_empty(), "{out}");
}
