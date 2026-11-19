//! The inbox line: notes left for the running chunk, said at session start.

use super::{fael, json, repo};

/// Several notes waiting at session start are one inbox line: one header and
/// one reply/show tail, a bullet per note — not a line (and tail) per note.
#[test]
fn several_inbox_notes_share_one_tail() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["chunk", "add", "T", "--brief", "b"], "");
    assert!(ok, "{err}");
    let uid = out.trim().to_string();
    let (ok, _, err) = fael(&d, &["chunk", "start", &uid, "--run", "R1"], "");
    assert!(ok, "{err}");
    for text in ["first word", "second word", "third word"] {
        let (ok, _, err) = super::fael_env(
            &d,
            &["chunk", "note", &uid, text],
            "",
            &[("FAEL_SESSION", "other")],
        );
        assert!(ok, "{err}");
    }
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","source":"startup"}}"#,
        json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err} {out}");
    for w in ["first word", "second word", "third word"] {
        assert!(out.contains(w), "{w} listed: {out}");
    }
    assert!(
        out.contains(&format!("3 new notes for chunk {uid}")),
        "{out}"
    );
    // the chunk line's pointer and the inbox tail: no third copy per note
    assert_eq!(out.matches("fael chunk show").count(), 2, "{out}");
}
