//! `fael chunk approve|unapprove|accept|show` over several uids, and `chunk list|ls`
//! (plan fael-friction-2 chunk-verbs): every uid runs in order, one result line per
//! uid, a failed one never stops the rest; `list` names the reads that do.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-chunk-verbs-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    d
}

fn hinted(d: &Path) -> String {
    let (ok, out, err) = fael(
        &d,
        &["chunk", "add", "T", "--brief", "b", "--model", "haiku"],
    );
    assert!(ok, "{err}");
    out.trim().to_string()
}

/// Three hinted chunks in one call: all approved, one result line each.
#[test]
fn approve_several_uids_approves_each_in_order() {
    let d = repo();
    let (a, b, c) = (hinted(&d), hinted(&d), hinted(&d));
    let (ok, out, err) = fael(&d, &["chunk", "approve", &a, &b, &c]);
    assert!(ok, "{err}");
    for u in [&a, &b, &c] {
        assert!(
            out.contains(&format!("chunk {u} → approved · model haiku")),
            "{out}"
        );
    }
}

/// Flags apply to every uid: one `--model` for two chunks.
#[test]
fn approve_flags_apply_to_every_uid() {
    let d = repo();
    let (x, y) = (hinted(&d), hinted(&d));
    let (ok, out, err) = fael(&d, &["chunk", "approve", &x, &y, "--model", "opus"]);
    assert!(ok, "{err}");
    for u in [&x, &y] {
        assert!(
            out.contains(&format!("chunk {u} → approved · model opus")),
            "{out}"
        );
    }
}

/// One bad uid among two good ones: both land, the bad one rejects alone, exit 1.
#[test]
fn a_bad_uid_rejects_alone_while_the_rest_land() {
    let d = repo();
    let (x, y) = (hinted(&d), hinted(&d));
    let bad = "01M59999999999999999999999";
    let (ok, out, err) = fael(&d, &["chunk", "approve", &x, bad, &y]);
    assert!(!ok, "a failed uid exits non-zero");
    assert!(out.contains(&format!("chunk {x} → approved")), "{out}");
    assert!(out.contains(&format!("chunk {y} → approved")), "{out}");
    assert!(out.contains(bad) && out.contains("no chunk"), "{out}");
    assert!(err.contains("1 of 3 chunks rejected"), "{err}");
}

/// `unapprove` and `show` take several uids too.
#[test]
fn unapprove_and_show_take_several_uids() {
    let d = repo();
    let (a, b) = (hinted(&d), hinted(&d));
    let (ok, out, err) = fael(&d, &["chunk", "unapprove", &a, &b]);
    assert!(ok, "{err}");
    assert!(out.contains(&format!("chunk {a} → unapproved")), "{out}");
    assert!(out.contains(&format!("chunk {b} → unapproved")), "{out}");
    let (ok, out, err) = fael(&d, &["chunk", "show", &a, &b]);
    assert!(ok, "{err}");
    assert!(out.contains(&a) && out.contains(&b), "{out}");
}

/// `list` is no verb: the board lists a plan's chunks, the export their briefs.
#[test]
fn list_names_the_reads_that_list_chunks() {
    let d = repo();
    for verb in ["list", "ls"] {
        let (ok, _, err) = fael(&d, &["chunk", verb]);
        assert!(!ok, "chunk {verb} stays a reject");
        assert!(err.contains("fael board --plan <plan>"), "{err}");
        assert!(err.contains("fael plan export <plan>"), "{err}");
    }
}
