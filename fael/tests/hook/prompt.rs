//! UserPromptSubmit says nothing: the key pointer it used to say earned 22%
//! and was cut (decision 01M5528X). The adapter is now a silent no-op.

use super::{fael, json, repo};

#[test]
fn a_prompt_naming_an_open_key_says_nothing() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let add = [
        "add",
        "decision",
        "append-only",
        "--files",
        "src/a.rs",
        "--key",
        "vela:credit-ledger",
    ];
    let (ok, _, err) = fael(&d, &add, "");
    assert!(ok, "{err}");
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","prompt":"vela:credit-ledger — is there credit ledger code yet?"}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "prompt", "--client", "claude"], &input);
    assert!(ok && out.is_empty(), "{out}");
}

/// 01M3XKB7H: a changed UserPromptSubmit payload must not switch the hint off
/// silently — the parse failure goes to stderr, the reply still fails open.
#[test]
fn prompt_hook_cries_about_an_unparsable_payload() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["hook", "prompt", "--client", "claude"], "{oops");
    assert!(
        ok && out.is_empty() && err.contains("unparsable"),
        "{out} {err}"
    );
}

/// 01M3XKB7H: serde defaults every field, so a renamed `prompt` parses Ok with
/// empty text — that shape change is logged too, not swallowed.
#[test]
fn prompt_hook_cries_about_a_missing_prompt_field() {
    let d = repo();
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","prompt_text":"credit"}}"#,
        json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "prompt", "--client", "claude"], &input);
    assert!(
        ok && out.is_empty() && err.contains("no `prompt` text"),
        "{out} {err}"
    );
}

#[test]
fn codex_prompts_remain_silent_across_turns() {
    let d = repo();
    for turn in ["turn-1", "turn-2"] {
        let input = serde_json::json!({
            "cwd": d, "session_id": "codex-session",
            "hook_event_name": "UserPromptSubmit", "turn_id": turn,
            "transcript_path": null, "model": "test-model",
            "permission_mode": "default", "prompt": "continue"
        })
        .to_string();
        let (ok, out, err) = fael(&d, &["hook", "prompt", "--client", "codex"], &input);
        assert!(ok && out.is_empty() && err.is_empty(), "{out} {err}");
    }
}
