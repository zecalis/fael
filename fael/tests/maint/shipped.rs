//! `doctor` for shipped notes: open notes on branches that already merged
//! (squash-safe: judged by branch name + `mergedAt`, never by sha).

use super::{fael, repo, state_env};
use std::path::Path;
use std::process::Command;

fn git(d: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// The temp repo's first branch is whatever `git init` made — shipped's
/// `git branch --merged` reads the default branch, so pin it to `main`.
fn pin_main(d: &Path) {
    git(d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let o = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(d)
        .output()
        .unwrap();
    if String::from_utf8_lossy(&o.stdout).trim() != "main" {
        git(d, &["branch", "-M", "main"]);
    }
}

/// `fael doctor` with canned `gh pr list --state merged` output through
/// `FAEL_GH_MERGED_JSON` (same reason as orphan's `FAEL_GH_JSON`: no
/// shell/batch fake survives Windows or real-gh runners).
fn doctor_args(d: &Path, gh_json: &str, args: &[&str]) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(d);
    state_env(&mut c, d);
    c.env("FAEL_GH_MERGED_JSON", gh_json);
    let o = c.output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

fn doctor(d: &Path, gh_json: &str) -> (bool, String) {
    doctor_args(d, gh_json, &["doctor"])
}

/// A note plus a decision filed on `branch` (the stamp comes from git).
/// Decisions never count — only `kind = note` ships.
fn file_rows(d: &Path, branch: &str) {
    git(d, &["checkout", "-qb", branch]);
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, _, err) = fael(
        d,
        &[
            "add",
            "note",
            "chunk 1 done: landed work",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let (ok, _, err) = fael(
        d,
        &["add", "decision", "landed choice", "--files", "src/a.rs"],
    );
    assert!(ok, "{err}");
    let (ok, _, _) = fael(d, &["doctor", "--fix"]);
    assert!(ok);
}

#[test]
fn doctor_flags_shipped_notes() {
    let d = repo();
    file_rows(&d, "feat/shipped-work");
    // squash fixture (§3): the row's sha never reaches main, but the branch
    // name plus a `mergedAt` after the row's birth prove it landed
    let (ok, out) = doctor(
        &d,
        r#"[{"headRefName":"feat/shipped-work","mergedAt":"2099-01-01T00:00:00Z","number":43}]"#,
    );
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Shipped] [--fix]: 1 open note(s)")
            && out.contains("feat/shipped-work")
            && out.contains("shipped in #43")
            && out.contains("fael close"),
        "{out}"
    );
    // a branch with no merged PR is not flagged
    let (ok, out) = doctor(
        &d,
        r#"[{"headRefName":"feat/other","mergedAt":"2099-01-01T00:00:00Z"}]"#,
    );
    assert!(ok && !out.contains("[Shipped"), "{out}");
    // no merged PR at all: silent
    let (ok, out) = doctor(&d, "[]");
    assert!(ok && !out.contains("[Shipped"), "{out}");
    // unparseable answer: skipped silently
    let (ok, out) = doctor(&d, "not json");
    assert!(ok && !out.contains("[Shipped"), "{out}");
}

#[test]
fn doctor_fix_closes_shipped_notes() {
    let d = repo();
    let fixture =
        r#"[{"headRefName":"feat/shipped-work","mergedAt":"2099-01-01T00:00:00Z","number":43}]"#;
    file_rows(&d, "feat/shipped-work");
    // same evidence as the report, but --fix applies the mechanical close
    let (ok, out) = doctor_args(&d, fixture, &["doctor", "--fix"]);
    assert!(ok, "{out}");
    assert!(
        out.contains("fixed: closed") && out.contains("shipped in #43"),
        "{out}"
    );
    // closed now: no [Shipped] on the next doctor, with or without the gh answer
    let (ok, out) = doctor(&d, fixture);
    assert!(ok && !out.contains("[Shipped"), "{out}");
    let (ok, out) = doctor(&d, "[]");
    assert!(ok && !out.contains("[Shipped"), "{out}");
    // the note is closed (no longer open); the decision beside it is untouched
    let (_, out, _) = fael(&d, &["find", "--kind", "note"]);
    assert!(!out.contains("landed work"), "{out}");
    let (_, out, _) = fael(&d, &["find", "--kind", "decision"]);
    assert!(out.contains("landed choice"), "{out}");
}

#[test]
fn doctor_silent_for_reused_branch_name() {
    let d = repo();
    file_rows(&d, "feat/reused");
    // every timed PR predates the row: a new branch under an old name
    let (ok, out) = doctor(
        &d,
        r#"[{"headRefName":"feat/reused","mergedAt":"2000-01-01T00:00:00Z","number":41}]"#,
    );
    assert!(ok && !out.contains("[Shipped"), "{out}");
}

#[test]
fn doctor_flags_shipped_maybe_from_git_only() {
    let d = repo();
    pin_main(&d);
    file_rows(&d, "feat/landed-note");
    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["merge", "-q", "--no-ff", "feat/landed-note"]);
    // no PR on record: the branch is merged locally but there is no merge
    // time, so the note is unconfirmed, never `[Shipped]`
    let (ok, out) = doctor(&d, "[]");
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Shipped?]: 1 open note(s)")
            && out.contains("feat/landed-note")
            && !out.contains("[Shipped]:"),
        "{out}"
    );
}

#[test]
fn doctor_silent_for_note_on_default_branch() {
    let d = repo();
    pin_main(&d);
    // a note filed on the default branch: the branch is always merged into
    // itself, so it must not read as work that shipped on a branch
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "on main", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    let (ok, out) = doctor(&d, "[]");
    assert!(ok, "{out}");
    assert!(!out.contains("[Shipped"), "{out}");
}

#[test]
fn handoff_and_revisit_notes_never_ship() {
    let d = repo();
    git(&d, &["checkout", "-qb", "feat/plan-chunk"]);
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    for extra in [
        &["--key", "plan:x:handoff"][..],
        &["--revisit=after the deploy"],
    ] {
        let mut args = vec!["add", "note", "waits past the merge", "--files", "src/a.rs"];
        args.extend_from_slice(extra);
        let (ok, _, err) = fael(&d, &args);
        assert!(ok, "{err}");
    }
    let gh = r#"[{"headRefName":"feat/plan-chunk","mergedAt":"2099-01-01T00:00:00Z","number":7}]"#;
    // --fix closes neither (it also repairs the fresh repo's `.gitattributes`)
    let (ok, out) = doctor_args(&d, gh, &["doctor", "--fix"]);
    assert!(ok && !out.contains("fixed: closed"), "{out}");
    let (ok, out) = doctor(&d, gh);
    assert!(ok && !out.contains("[Shipped"), "{out}");
}

/// The shapes named in 01M4164E: standing rules and facts filed from a branch
/// that later landed, plus a `half done` note — none is a status of the work.
const KEPT: &[&str] = &[
    "Benchmark rows removed from usage.jsonl; benchmarks must isolate state. Rule: point FAEL_STATE_DIR at a scratch dir.",
    "Worktrees keep their own target dir. Do NOT set CARGO_TARGET_DIR to a shared dir.",
    "Verify branch with git branch --show-current before git push, as happened when a commit landed on fix/other instead",
    "OpenCode exports no session env var to shell or MCP (read from the bundled code, not yet run).",
    "The earlier row is half done on this branch: the rule was left out",
];

#[test]
fn doctor_lists_but_never_closes_rules_and_facts() {
    let d = repo();
    let gh = r#"[{"headRefName":"feat/mixed","mergedAt":"2099-01-01T00:00:00Z","number":50}]"#;
    git(&d, &["checkout", "-qb", "feat/mixed"]);
    let status = "PR opened for the doctor fix, branch feat/mixed";
    // one file per row: a shared file would let add's self-heal supersede them
    for (i, text) in KEPT.iter().copied().chain([status]).enumerate() {
        let file = format!("src/f{i}.rs");
        std::fs::write(d.join(&file), "").unwrap();
        let (ok, _, err) = fael(&d, &["add", "note", text, "--files", &file]);
        assert!(ok, "{err}");
    }
    // repair the fresh repo's `.gitattributes` first, so only [Shipped*] is left
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    // the report names what it closes and what it leaves alone, separately
    let (ok, out) = doctor(&d, gh);
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Shipped] [--fix]: 1 open note(s)")
            && out.contains("note [Shipped kept]: 5 open note(s)")
            && out.contains("`--fix` never closes them"),
        "{out}"
    );
    // --fix closes only the status note; every rule/fact stays open
    let (ok, out) = doctor_args(&d, gh, &["doctor", "--fix"]);
    assert!(ok && out.contains("shipped in #50"), "{out}");
    assert_eq!(out.matches("fixed: closed").count(), 1, "{out}");
    let (_, out, _) = fael(&d, &["find", "--kind", "note"]);
    assert!(!out.contains("PR opened"), "{out}");
    for text in KEPT {
        let head = text.split(['.', ':', ';', ',']).next().unwrap();
        assert!(out.contains(head), "{head}: {out}");
    }
    // still listed afterwards, still not closed by a second --fix
    let (ok, out) = doctor_args(&d, gh, &["doctor", "--fix"]);
    assert!(
        ok && out.contains("note [Shipped kept]: 5 open note(s)") && !out.contains("fixed: closed"),
        "{out}"
    );
}
