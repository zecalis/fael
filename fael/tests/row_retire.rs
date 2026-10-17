//! Row lifecycle D1 (decision 01M5528X): a note tied to a chunk or a branch retires when
//! `fael chunk done` ends that chunk or git proves the branch merged; an unmerged branch's
//! note stays. Retired rows are hidden like closed and shown `(retired)` under `--all`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(d: &Path, state: &Path, session: Option<&str>, args: &[&str]) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID");
    if let Some(s) = session {
        c.env("CLAUDE_CODE_SESSION_ID", s);
    }
    let o = c.output().unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(d: &Path, args: &[&str]) {
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    let ok = Command::new("git")
        .args(id)
        .args(args)
        .current_dir(d)
        .status()
        .unwrap();
    assert!(ok.success(), "git {args:?}");
}

fn repo() -> (PathBuf, PathBuf) {
    let t = std::env::temp_dir().join(format!("fael-retire-{}", fael_core::ulid()));
    let d = t.join("r");
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    (d.canonicalize().unwrap(), t.join("state"))
}

#[test]
fn a_note_retires_when_its_chunk_is_done() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    git(&d, &["switch", "-qc", "feat/x"]);
    f(&["add", "note", "stopped at step two", "--files", "plan:demo"]);
    let a = f(&["chunk", "add", "a", "--brief", "b"]);
    f(&["chunk", "start", &a, "--run", "R1"]);
    assert!(f(&["find", "stopped"]).contains("stopped at step two"));
    f(&["chunk", "done", &a, "h", "--run", "R1"]);
    assert!(f(&["find", "stopped"]).starts_with("no open row matches"));
    let all = f(&["find", "stopped", "--all"]);
    assert!(all.contains("(retired)"), "{all}");
}

#[test]
fn a_handoff_note_with_no_stamped_session_stays() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, Some("s1"), a);
    f(&["plan", "new", "demo", "Demo"]);
    f(&[
        "add",
        "note",
        "where it stopped",
        "--files",
        "plan:demo",
        "--key",
        "plan:demo:handoff",
    ]);
    let a = f(&["chunk", "add", "a", "--plan", "demo", "--brief", "b"]);
    f(&["chunk", "start", &a, "--run", "R1", "--no-worktree"]);
    f(&["chunk", "done", &a, "h", "--run", "R1"]);
    // the run's session is stamped by the Stop hook, not by start; here: unproven, stays
    assert!(f(&["find", "where it stopped"]).contains("where it stopped"));
}

#[test]
fn a_note_retires_when_its_branch_is_merged_and_not_before() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    git(&d, &["switch", "-qc", "feat/x"]);
    f(&[
        "add",
        "note",
        "uncommitted on feat/x",
        "--files",
        "plan:demo",
    ]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    git(&d, &["switch", "-q", "main"]);
    assert!(f(&["find", "uncommitted"]).contains("uncommitted on feat/x"));
    git(&d, &["merge", "-q", "--squash", "feat/x"]);
    git(&d, &["commit", "-qm", "squash"]);
    assert!(f(&["find", "uncommitted"]).starts_with("no open row matches"));
    assert!(f(&["find", "uncommitted", "--all"]).contains("(retired)"));
}

#[test]
fn stats_counts_rows_by_state_per_kind() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    std::fs::write(d.join("a.rs"), "1\n").unwrap();
    std::fs::write(d.join("b.rs"), "1\n").unwrap();
    f(&[
        "add",
        "decision",
        "all changed",
        "--title",
        "t one",
        "--files",
        "a.rs",
    ]);
    f(&[
        "add",
        "decision",
        "some changed",
        "--title",
        "t two",
        "--files",
        "a.rs,b.rs",
    ]);
    f(&[
        "add",
        "decision",
        "none changed",
        "--title",
        "t three",
        "--files",
        "b.rs",
    ]);
    f(&[
        "add", "issue", "gone", "--title", "t four", "--files", "b.rs",
    ]);
    let listed = f(&["find", "--kind", "issue"]);
    let id = listed.split(['[', ']']).nth(1).unwrap().to_string();
    f(&["close", &id, "done"]);
    std::fs::write(d.join("a.rs"), "2\n").unwrap();
    let out = f(&["stats", "--json"]);
    let l: serde_json::Value = serde_json::from_str(&out).unwrap();
    let l = &l["lifecycle"];
    assert_eq!(l["decision"]["dormant"], 1, "{l}");
    assert_eq!(l["decision"]["held"], 1, "{l}");
    assert_eq!(l["decision"]["live"], 1, "{l}");
    assert_eq!(l["issue"]["retired"], 1, "{l}");
    assert!(f(&["stats"]).starts_with("rows (this repo):\n  decision: live 1 · held 1"));
}
