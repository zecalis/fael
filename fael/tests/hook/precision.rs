//! Push precision fixture, binary layer (PLAN-fael-moat-token chunk 2): what
//! the edit push renders around the rows — the count footer and the
//! `@branch` tag — for the real-session cases of notes 01M3S446P / 01M3S6NAH.

use super::{fael, json, repo};
use std::path::Path;

fn edit(d: &Path, file: &str) -> String {
    let input = format!(r#"{{"cwd":{},"files":["{file}"]}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "edit"], &input);
    assert!(ok, "{err}");
    out
}

/// Footer lines of a push reply (the reply is one JSON line, `\n` escaped).
fn footer(out: &str) -> Vec<&str> {
    out.split("\\n").filter(|l| l.starts_with("… +")).collect()
}

/// Case "footer" (01M3S4EBF): a file whose rows carry many keys. Eight rows
/// on `src/a.rs`, each with its own key, each key shared with a row on another
/// file. With no key in the session Focus the siblings are not counted at all
/// (decision push:shared-key-siblings), and no footer line is said.
#[test]
fn shared_keys_outside_focus_are_not_counted() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    for i in 0..8 {
        // one file per note — same kind on one file would supersede itself
        for (kind, f) in [
            ("decision", "src/a.rs".to_string()),
            ("note", format!("lib/z{i}.rs")),
        ] {
            let (ok, _, err) = fael(
                &d,
                &[
                    "add",
                    kind,
                    &format!("{kind} {i} about topic number {i}"),
                    "--files",
                    &f,
                    "--key",
                    &format!("topic:t{i}"),
                ],
                "",
            );
            assert!(ok, "{err}");
        }
    }
    let out = edit(&d, "src/a.rs");
    assert!(footer(&out).is_empty(), "{out}");
    assert!(out.contains("(5 of 8):"), "{out}");
}
