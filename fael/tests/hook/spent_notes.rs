//! A note leaves the edit push after five sessions (src/hook/spent_notes.rs):
//! the fifth also says so once; a decision on the same file keeps being said.

use super::changed_hint::{add, edit_or_silent};
use super::{fael, repo};

#[test]
fn a_note_is_said_in_five_sessions_then_retired_but_a_decision_is_not() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let note = add(&d, "note", "A2 done, B3 next", "src/a.rs");
    add(&d, "decision", "retry uses backoff here", "src/a.rs");
    let retired = |out: &str| out.contains("retired from the push");
    for i in 1..=5 {
        let out = edit_or_silent(&d, &format!("s{i}"), "src/a.rs");
        assert!(out.contains("A2 done, B3 next"), "s{i}: {out}");
        assert_eq!(retired(&out), i == 5, "s{i}: {out}");
        if i == 5 {
            assert!(out.contains("fael add decision"), "{out}");
            assert!(out.contains("--supersedes"), "{out}");
        }
    }
    let six = edit_or_silent(&d, "s6", "src/a.rs");
    assert!(!six.contains("A2 done"), "{six}");
    assert!(!retired(&six), "{six}");
    assert!(six.contains("retry uses backoff here"), "{six}");
    // a second edit in the fifth session does not say the line again
    let again = edit_or_silent(&d, "s5", "src/a.rs");
    assert!(!retired(&again), "{again}");
    // the note is still in the log
    let (ok, out, err) = fael(&d, &["find", "--all", "A2 done"], "");
    assert!(ok, "{err}");
    assert!(out.contains(&note[..8]) || out.contains("A2 done"), "{out}");
}
