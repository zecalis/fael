//! `kickoff` marks a branch tag ` (merged)` once that branch's work is in
//! HEAD — squash merges included — and never for a branch that has no
//! commits of its own. `find plan:x` reads the anchor like `kickoff plan:x`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(d)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn fael(d: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        // never the developer's real usage log or session (fael:01M4F3G0)
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join(format!("fael-test-state-{}", std::process::id())),
        )
        .env_remove("CLAUDE_CODE_SESSION_ID").env_remove("FAEL_SESSION").env_remove("CODEX_THREAD_ID")
        .env_remove("FAEL_SESSION")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-kickoff-merged-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["config", "user.name", "Merge Test"]);
    git(&d, &["config", "user.email", "merge@example.com"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    d
}

/// The kickoff line of the row whose text holds `needle`.
fn line<'a>(out: &'a str, needle: &str) -> &'a str {
    out.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in:\n{out}"))
}

#[test]
fn kickoff_marks_squash_merged_branch_and_not_an_empty_one() {
    let d = repo();
    git(&d, &["switch", "-qc", "feat/x"]);
    fael(
        &d,
        &[
            "add",
            "note",
            "uncommitted on feat/x",
            "--files",
            "plan:demo",
        ],
    );
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["switch", "-qc", "feat/empty"]);
    fael(
        &d,
        &[
            "add",
            "note",
            "cut but never committed",
            "--files",
            "plan:demo",
        ],
    );
    git(&d, &["switch", "-q", "main"]);

    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(line(&out, "uncommitted on").ends_with("@feat/x"), "{out}");
    assert!(
        line(&out, "never committed").ends_with("@feat/empty"),
        "{out}"
    );

    git(&d, &["merge", "-q", "--squash", "feat/x"]);
    git(&d, &["commit", "-qm", "squash feat/x"]);
    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(
        line(&out, "uncommitted on").ends_with("@feat/x (merged)"),
        "{out}"
    );
    // still on main's first-parent line: cut from it, never merged
    assert!(
        line(&out, "never committed").ends_with("@feat/empty"),
        "{out}"
    );
    // the tag is kickoff's: find stays free of git spawns
    assert!(!fael(&d, &["find", "plan:demo"]).contains("(merged)"));
}

#[test]
fn kickoff_keeps_the_mark_when_a_tag_shares_the_dead_branchs_name() {
    let d = repo();
    git(&d, &["switch", "-qc", "feat/tag"]);
    fael(
        &d,
        &[
            "add",
            "note",
            "tag shadows the branch",
            "--files",
            "plan:demo",
        ],
    );
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    let tip = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "feat/tag"])
            .current_dir(&d)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["merge", "-q", "--squash", "feat/tag"]);
    git(&d, &["commit", "-qm", "squash feat/tag"]);
    git(&d, &["branch", "-D", "feat/tag"]);
    // the branch is gone, but a tag of the same name resolves the same tip
    // through `rev-parse <b>`: the gate must not read that as dead
    git(&d, &["tag", "feat/tag", tip.trim()]);

    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(
        line(&out, "tag shadows").ends_with("@feat/tag (merged)"),
        "{out}"
    );
}

#[test]
fn kickoff_keeps_the_mark_when_the_branch_name_is_a_pseudo_ref() {
    // git allows a branch named after a pseudo-ref, and `$GIT_DIR/FETCH_HEAD`
    // outlives it: `rev-parse FETCH_HEAD` still proves the tip.
    let d = repo();
    git(&d, &["switch", "-qc", "FETCH_HEAD"]);
    fael(
        &d,
        &[
            "add",
            "note",
            "on a pseudo-ref name",
            "--files",
            "plan:demo",
        ],
    );
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    git(&d, &["fetch", "-q", ".", "refs/heads/FETCH_HEAD"]);
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["merge", "-q", "--squash", "FETCH_HEAD"]);
    git(&d, &["commit", "-qm", "squash FETCH_HEAD"]);
    git(&d, &["branch", "-D", "FETCH_HEAD"]);

    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(
        line(&out, "pseudo-ref name").ends_with("@FETCH_HEAD (merged)"),
        "{out}"
    );
}

#[test]
fn kickoff_reads_the_packed_table_past_a_non_utf8_refname() {
    let d = repo();
    git(&d, &["switch", "-qc", "feat/packed"]);
    fael(&d, &["add", "note", "packed only", "--files", "plan:demo"]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    let tip = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "feat/packed"])
            .current_dir(&d)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["merge", "-q", "--squash", "feat/packed"]);
    git(&d, &["commit", "-qm", "squash feat/packed"]);
    git(&d, &["branch", "-D", "feat/packed"]);
    // a tag of the same name keeps the tip resolvable, and packing it leaves
    // the packed table as the only place that can prove it
    git(&d, &["tag", "feat/packed", tip.trim()]);
    git(&d, &["pack-refs", "--all", "--prune"]);
    assert!(
        !d.join(".git/refs/tags/feat/packed").exists(),
        "tag still loose"
    );
    // one refname with a raw non-UTF-8 byte must not blind the other lines
    let mut packed = std::fs::OpenOptions::new()
        .append(true)
        .open(d.join(".git/packed-refs"))
        .unwrap();
    packed.write_all(tip.trim().as_bytes()).unwrap();
    packed.write_all(b" refs/heads/bad-\xff\n").unwrap();

    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(
        line(&out, "packed only").ends_with("@feat/packed (merged)"),
        "{out}"
    );
}

#[test]
fn kickoff_marks_origin_only_branch_as_merged() {
    let d = repo();
    git(&d, &["switch", "-qc", "feat/origin-only"]);
    fael(
        &d,
        &[
            "add",
            "note",
            "uncommitted on feat/origin-only",
            "--files",
            "plan:demo",
        ],
    );
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["merge", "-q", "--squash", "feat/origin-only"]);
    git(&d, &["commit", "-qm", "squash feat/origin-only"]);

    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(
        line(&out, "uncommitted on").ends_with("@feat/origin-only (merged)"),
        "{out}"
    );

    // the local branch is gone but our fetch of it is still live: the
    // spawn-free gate must let it through to `rev-parse origin/<b>`, or the
    // mark is lost.
    let tip = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "feat/origin-only"])
            .current_dir(&d)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    git(&d, &["branch", "-D", "feat/origin-only"]);
    let origin_ref = d.join(".git/refs/remotes/origin/feat/origin-only");
    std::fs::create_dir_all(origin_ref.parent().unwrap()).unwrap();
    std::fs::write(&origin_ref, &tip).unwrap();

    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(
        line(&out, "uncommitted on").ends_with("@feat/origin-only (merged)"),
        "{out}"
    );
}

#[test]
fn find_reads_an_anchor_like_kickoff() {
    let d = repo();
    fael(
        &d,
        &["add", "note", "handoff for demo", "--files", "plan:demo"],
    );
    let out = fael(&d, &["find", "plan:demo"]);
    assert!(out.contains("handoff for demo"), "{out}");
    // a name no row is filed on stays a text search
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        // never the developer's real usage log or session (fael:01M4F3G0)
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join(format!("fael-test-state-{}", std::process::id())),
        )
        .env_remove("CLAUDE_CODE_SESSION_ID").env_remove("FAEL_SESSION").env_remove("CODEX_THREAD_ID")
        .env_remove("FAEL_SESSION")
        .args(["find", "plan:other"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("no rows match \"plan:other\""));
}

#[test]
fn find_reads_a_key_like_find_key() {
    // agents copy the `#key` a row prints, or the `key:` field name; the body
    // never holds the key, so only a key lookup can find it
    let d = repo();
    fael(
        &d,
        &[
            "add",
            "note",
            "m2 done, m2b is left",
            "--files",
            "a.rs",
            "--key",
            "vela:money-m2",
        ],
    );
    for q in ["vela:money-m2", "#vela:money-m2", "key:vela:money-m2"] {
        let out = fael(&d, &["find", q]);
        assert!(out.contains("m2b is left"), "{q}: {out}");
    }
    // a bare word that is also a key stays a text search
    fael(
        &d,
        &[
            "add",
            "note",
            "usage log is append-only",
            "--files",
            "a.rs",
            "--key",
            "usage",
        ],
    );
    fael(
        &d,
        &[
            "add",
            "note",
            "usage rows carry a session",
            "--files",
            "b.rs",
        ],
    );
    let out = fael(&d, &["find", "usage"]);
    assert!(out.contains("carry a session"), "{out}");
}
