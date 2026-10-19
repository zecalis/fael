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
        d,
        &["chunk", "add", "T", "--brief", "b", "--model", "haiku"],
    );
    assert!(ok, "{err}");
    out.trim().to_string()
}

fn plan(d: &Path, name: &str) {
    let (ok, _, err) = fael(d, &["plan", "new", name, "P"]);
    assert!(ok, "{err}");
}

/// A hinted chunk with `label` in `plan`, its uid.
fn labeled(d: &Path, plan: &str, label: &str) -> String {
    let (ok, out, err) = fael(
        d,
        &[
            "chunk", "add", "T", "--plan", plan, "--label", label, "--brief", "b", "--model",
            "haiku",
        ],
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

/// `--plan` resolves each key on its own: one bad label rejects alone, the rest land.
#[test]
fn a_bad_plan_label_rejects_alone_while_the_rest_land() {
    let d = repo();
    plan(&d, "p");
    let (a, c) = (labeled(&d, "p", "a"), labeled(&d, "p", "c"));
    let (ok, out, err) = fael(&d, &["chunk", "approve", "--plan", "p", "a", "nope", "c"]);
    assert!(!ok, "a failed key exits non-zero");
    assert!(out.contains(&format!("chunk {a} → approved")), "{out}");
    assert!(out.contains(&format!("chunk {c} → approved")), "{out}");
    assert!(out.contains("no chunk nope"), "{out}");
    assert!(err.contains("1 of 3 chunks rejected"), "{err}");
}

/// `--plan` never widens: a uid in another plan is refused, the rest still run.
#[test]
fn a_key_in_another_plan_rejects_alone() {
    let d = repo();
    plan(&d, "p");
    plan(&d, "q");
    let (a, c) = (labeled(&d, "p", "a"), labeled(&d, "p", "c"));
    let q = labeled(&d, "q", "z");
    let (ok, out, err) = fael(&d, &["chunk", "approve", "--plan", "p", "a", &q, "c"]);
    assert!(!ok, "a failed key exits non-zero");
    assert!(out.contains(&format!("chunk {a} → approved")), "{out}");
    assert!(out.contains(&format!("chunk {c} → approved")), "{out}");
    assert!(out.contains("is in plan q, not p"), "{out}");
    assert!(err.contains("1 of 3 chunks rejected"), "{err}");
}

/// `show --plan` takes several labels too: a bad one is its own line, the rest show.
#[test]
fn show_takes_several_labels_with_a_plan() {
    let d = repo();
    plan(&d, "p");
    let (a, b) = (labeled(&d, "p", "a"), labeled(&d, "p", "b"));
    let (ok, out, err) = fael(&d, &["chunk", "show", "--plan", "p", "a", "nope", "b"]);
    assert!(!ok, "a failed key exits non-zero");
    assert!(out.contains(&a) && out.contains(&b), "{out}");
    assert!(out.contains("no chunk nope"), "{out}");
    assert!(err.contains("1 of 3 chunks rejected"), "{err}");
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

/// An unknown verb names itself and the short guide, never the whole verb list.
#[test]
fn an_unknown_verb_names_itself_and_the_short_guide() {
    let d = repo();
    plan(&d, "p");
    let (ok, _, err) = fael(&d, &["chunk", "frobnicate"]);
    assert!(!ok, "an unknown verb stays a reject");
    assert!(err.contains("unknown chunk verb \"frobnicate\""), "{err}");
    assert!(
        err.contains("fael chunk — the units of work in a plan"),
        "{err}"
    );
    assert!(!err.contains("unapprove"), "{err}");
}

/// A near-miss verb offers the closest real one.
#[test]
fn a_near_miss_verb_suggests_the_closest_one() {
    let d = repo();
    plan(&d, "p");
    let (ok, _, err) = fael(&d, &["chunk", "shwo"]);
    assert!(!ok, "a near-miss verb stays a reject");
    assert!(err.contains("did you mean show?"), "{err}");
}
