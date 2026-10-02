//! UserPromptSubmit: a prompt naming an open key gets one pointer line,
//! once per session; nothing named = nothing printed.

use super::{fael, fael_env, json, repo};

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
        ok && out.contains("UserPromptSubmit")
            // 01M3XKB7G: the line names the word that fired, so a misfire is
            // judgeable from the line and from the usage journal
            && out.contains("vela:credit-ledger (1, via \\\"credit\\\")"),
        "{out}"
    );
    // pointer only — the row itself never rides the prompt
    assert!(!out.contains("append-only"), "{out}");
    // once per session
    let (ok, out, _) = ask("s1", "credit again");
    assert!(ok && out.is_empty(), "{out}");
    // a fresh session: no exact head = silent (a trailing segment is no
    // head), the exact one still points
    let (ok, out, _) = ask("s2", "credits ledger");
    assert!(ok && out.is_empty(), "{out}");
    let (ok, out, _) = ask("s2", "the credit layer");
    assert!(ok && out.contains("vela:credit-ledger"), "{out}");
    // no session = no once-only list, so no hint
    let (ok, out, _) = ask("", "credit");
    assert!(ok && out.is_empty(), "{out}");
}

/// 01M3XKB7M: a key whose open rows this session already read (its `.seen`
/// list, written by a push or `find --files`) is not pointed at again. The
/// key carries a superseded older version too, so this also pins the open-only
/// filter in `rows_all_seen` (a non-open sibling must not defeat the skip).
#[test]
fn prompt_skips_a_key_rows_already_seen() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let add = |args: &[&str]| {
        let (ok, out, err) = fael(&d, args, "");
        assert!(ok, "{err}");
        out
    };
    add(&[
        "add",
        "decision",
        "old ledger rule",
        "--files",
        "src/a.rs",
        "--key",
        "vela:credit-ledger",
    ]);
    let old = {
        let (ok, out, err) = fael(&d, &["find", "--key", "vela:credit-ledger"], "");
        assert!(ok, "{err}");
        let id = out
            .lines()
            .find_map(|l| l.strip_prefix("- [")?.split(']').next())
            .expect("the added row lists");
        id.to_string()
    };
    add(&[
        "add",
        "decision",
        "new ledger rule",
        "--files",
        "src/a.rs",
        "--key",
        "vela:credit-ledger",
        "--supersedes",
        &old,
    ]);
    let ask = |s: &str, p: &str| {
        let input = format!(
            r#"{{"cwd":{},"session_id":"{s}","prompt":{}}}"#,
            json(&d),
            serde_json::to_string(p).unwrap()
        );
        fael(&d, &["hook", "prompt", "--client", "claude"], &input)
    };
    // the session read the file: `find --files` shows the open row and notes
    // its id in `.seen` (a bare `--key` find records nothing)
    let (ok, _, err) = fael_env(
        &d,
        &["find", "--files", "src/a.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "s1")],
    );
    assert!(ok, "{err}");
    // not asked before, so `.keys` is empty — only the `.seen` skip can mute it
    let (ok, out, _) = ask("s1", "credit");
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
