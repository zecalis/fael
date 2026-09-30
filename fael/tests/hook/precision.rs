//! Push precision fixture, binary layer (PLAN-fael-moat-token chunk 2): what
//! the read push renders around the rows — the count footer and the
//! `@branch` tag — for the real-session cases of notes 01M3S446P / 01M3S6NAH.

#[cfg(unix)]
use super::git;
use super::{fael, json, repo};
use std::path::Path;

fn read(d: &Path, file: &str) -> String {
    let input = format!(r#"{{"cwd":{},"files":["{file}"]}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "read"], &input);
    assert!(ok, "{err}");
    out
}

/// Footer lines of a push reply (the reply is one JSON line, `\n` escaped).
fn footer(out: &str) -> Vec<&str> {
    out.split("\\n").filter(|l| l.starts_with("… +")).collect()
}

/// Case "footer" (01M3S4EBF): a file whose rows carry many keys. Eight rows
/// on `src/a.rs`, each with its own key, each key shared with a row on another
/// file — the count footer used to spend a line per key (1 file line + 8 key lines). Now: 2.
#[test]
fn many_hidden_keys_fold_into_one_footer_line() {
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
    let out = read(&d, "src/a.rs");
    let lines = footer(&out);
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert!(lines[0].contains("more about this file"), "{lines:#?}");
    assert!(
        lines[1].starts_with("… +8 more under 8 keys: #topic:t7 (1), #topic:t6 (1), #topic:t5 (1), +5 keys — fael find --key <key>"),
        "{lines:#?}"
    );
}

/// A `.fael` symlinked out of the checkout — the tree every worktree of the
/// clone shares, untouched by `git switch` (this repo's own setup).
#[cfg(unix)]
fn share_tree(d: &Path) {
    let shared = d.with_file_name(format!(
        "{}-shared",
        d.file_name().unwrap().to_string_lossy()
    ));
    // the fixture's `.fael` (its tracked pin) becomes the shared dir
    std::fs::rename(d.join(".fael"), &shared).unwrap();
    std::os::unix::fs::symlink(&shared, d.join(".fael")).unwrap();
}

/// Case "off-branch" (01M3S4EBE): a row filed on another branch that is
/// still alive reads as fact when the tree is shared. It used to read
/// untagged; now it carries `@feat/x` while that branch exists.
#[cfg(unix)]
#[test]
fn off_branch_row_in_a_shared_tree_is_tagged() {
    let d = repo();
    share_tree(&d);
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    git(&d, &["add", "src/a.rs"]);
    git(&d, &["commit", "-qm", "a"]);
    let main = git(&d, &["symbolic-ref", "--short", "HEAD"]);
    git(&d, &["switch", "-qc", "feat/x"]);
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "only on feat/x", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    git(&d, &["switch", "-q", &main]);
    let out = read(&d, "src/a.rs");
    assert!(out.contains("only on feat/x"), "{out}");
    assert!(out.contains("@feat/x"), "{out}");
}

/// The same row once its branch is gone (merged and deleted, or a throwaway
/// worktree branch): nothing to point at, so no tag — before and after.
#[cfg(unix)]
#[test]
fn off_branch_row_of_a_deleted_branch_in_the_tree_stays_untagged() {
    let d = repo();
    share_tree(&d);
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    git(&d, &["add", "src/a.rs"]);
    git(&d, &["commit", "-qm", "a"]);
    let main = git(&d, &["symbolic-ref", "--short", "HEAD"]);
    git(&d, &["switch", "-qc", "feat/x"]);
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "filed on feat/x", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    git(&d, &["switch", "-q", &main]);
    git(&d, &["branch", "-D", "feat/x"]);
    let out = read(&d, "src/a.rs");
    assert!(out.contains("filed on feat/x"), "{out}");
    assert!(!out.contains("@feat/x"), "{out}");
}
