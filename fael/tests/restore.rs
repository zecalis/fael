//! `fael restore` through the real binary: reopening by target or by edge,
//! idempotency (a repeat writes nothing), rejects, doctor precision after,
//! and the intended B+A-open → next-add-Holds behaviour (format.md §Restore).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Per-child `FAEL_STATE_DIR` at `<repo root>/state`, so a real session on
/// this machine never leaks in and tests run in parallel.
fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", root.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID");
    if !stdin.is_empty() {
        c.stdin(Stdio::piped());
    }
    let mut c = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if !stdin.is_empty() {
        c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    }
    let o = c.wait_with_output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-restore-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Restore Test"],
        &["config", "user.email", "restore@example.com"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&d)
                .status()
                .unwrap()
                .success()
        );
    }
    Command::new("git")
        .args(["commit", "-q", "--allow-empty", "-m", "init"])
        .current_dir(&d)
        .status()
        .unwrap();
    std::fs::write(d.join("src/a.rs"), "// a.rs\n").unwrap();
    d
}

/// The id `fael add` just filed (stdout starts with it).
fn add(dir: &Path, args: &[&str]) -> String {
    let (ok, out, err) = fael(dir, args, "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// Lines in the tree log (row stream only — the restore row lands there too).
fn log_lines(dir: &Path) -> usize {
    let mut n = 0;
    let log = dir.join(".fael/log");
    let mut stack = vec![log];
    while let Some(p) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&p) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.to_string_lossy().ends_with(".jsonl")
                && !p.to_string_lossy().ends_with(".close.jsonl")
            {
                n += std::fs::read_to_string(&p).unwrap().lines().count();
            }
        }
    }
    n
}

/// A listed row owns this id (`"id":"…"`) — a hidden row's id still shows
/// inside another row's `supersedes`, so a bare substring match lies.
fn lists(out: &str, id: &str) -> bool {
    out.contains(&format!("\"id\":\"{id}\""))
}

/// A hidden by B: add A, then supersede it with an explicit caller flag.
/// `--json` lists full ids (the human list abbreviates them).
fn superseded_pair(dir: &Path) -> (String, String) {
    let a = add(
        dir,
        &[
            "add",
            "issue",
            "login loops",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
        ],
    );
    let b = add(
        dir,
        &[
            "add",
            "note",
            "loops fixed",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
            "--supersedes",
            &a,
        ],
    );
    let (ok, out, err) = fael(dir, &["find", "--json", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    assert!(!lists(&out, &a), "A is hidden until restored: {out}");
    (a, b)
}

#[test]
fn restore_reopens_and_is_idempotent() {
    let d = repo();
    let (a, _) = superseded_pair(&d);
    let before = log_lines(&d);
    let (ok, out, err) = fael(&d, &["restore", &a], "");
    assert!(ok, "{err}");
    assert!(out.contains("restored"), "{out}");
    assert_eq!(log_lines(&d), before + 1, "one event row");
    let (ok, out, _) = fael(&d, &["find", "--json", "--files", "src/a.rs"], "");
    assert!(ok);
    assert!(lists(&out, &a), "A is open again: {out}");
    // a repeat is info, never an error, and writes nothing
    let (ok, out, _) = fael(&d, &["restore", &a], "");
    assert!(ok, "{out}");
    assert!(out.contains("already"), "{out}");
    assert_eq!(log_lines(&d), before + 1, "idempotent: no second row");
}

#[test]
fn close_superseder_after_restore_keeps_target_open() {
    // closing the row whose supersede edge was reverted must not sweep the
    // restored row into the close chain — a reverted edge hides nothing
    let d = repo();
    let (a, b) = superseded_pair(&d);
    let (ok, _, err) = fael(&d, &["restore", &a], "");
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["close", &b, "done"], "");
    assert!(ok, "{err}");
    let (ok, out, err) = fael(&d, &["find", "--json", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    assert!(lists(&out, &a), "A stays open after closing B: {out}");
}

#[test]
fn restore_names_edge_when_two_hide() {
    let d = repo();
    let (a, b) = superseded_pair(&d);
    let c = add(
        &d,
        &[
            "add",
            "note",
            "loops fixed again",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
            "--supersedes",
            &a,
        ],
    );
    let (ok, _, err) = fael(&d, &["restore", &a], "");
    assert!(!ok, "two active edges must not pick one silently");
    assert!(err.contains("--edge"), "{err}");
    // revert one edge: A still hidden behind the other
    let (ok, out, err) = fael(&d, &["restore", "--edge", &b], "");
    assert!(ok, "{err}");
    assert!(out.contains("restored"), "{out}");
    let (ok, out, _) = fael(&d, &["find", "--json", "--files", "src/a.rs"], "");
    assert!(ok);
    assert!(!lists(&out, &a), "C still hides A: {out}");
    // revert the last edge: A opens
    let (ok, _, err) = fael(&d, &["restore", "--edge", &c], "");
    assert!(ok, "{err}");
    let (ok, out, _) = fael(&d, &["find", "--json", "--files", "src/a.rs"], "");
    assert!(ok);
    assert!(lists(&out, &a), "{out}");
}

#[test]
fn restore_open_row_is_info() {
    let d = repo();
    let a = add(&d, &["add", "issue", "login loops", "--files", "src/a.rs"]);
    let before = log_lines(&d);
    let (ok, out, _) = fael(&d, &["restore", &a], "");
    assert!(ok);
    assert!(out.contains("already open"), "{out}");
    assert_eq!(log_lines(&d), before, "nothing written");
}

#[test]
fn restore_unknown_and_edgeless_reject() {
    let d = repo();
    let (a, b) = superseded_pair(&d);
    let (ok, _, err) = fael(&d, &["restore", "deadbeef"], "");
    assert!(!ok);
    assert!(err.contains("no row"), "{err}");
    // --edge on a row that supersedes nothing names no edge
    let (ok, _, err) = fael(&d, &["restore", "--edge", &a], "");
    assert!(!ok);
    assert!(err.contains("supersedes nothing"), "{err}");
    // --edge and target disagree
    let (ok, _, err) = fael(&d, &["restore", &b, "--edge", &b], "");
    assert!(!ok, "B supersedes A, so it cannot revert an edge into B");
    assert!(err.contains("not"), "{err}");
}

#[test]
fn doctor_reports_precision_after_restore() {
    let d = repo();
    let (a, _) = superseded_pair(&d);
    let (ok, _, err) = fael(&d, &["restore", &a], "");
    assert!(ok, "{err}");
    // the throwaway repo has no union line yet — fix that, then the only
    // note left is the restore label's per-rule precision (info-only, exit 0)
    let (ok, out, err) = fael(&d, &["doctor", "--fix"], "");
    assert!(ok, "{out} {err}");
    let (ok, out, err) = fael(&d, &["doctor"], "");
    assert!(ok, "{err}");
    assert!(out.contains("[Precision]"), "{out}");
    assert!(out.contains("caller:flag 0/1 correct"), "{out}");
    // the re-opened row carries its label in find too
    let (ok, out, _) = fael(&d, &["find", "--files", "src/a.rs"], "");
    assert!(ok);
    assert!(out.contains("(restored)"), "{out}");
}

#[test]
fn both_open_next_add_holds() {
    // after a restore both ends are open on one key — the next same-kind add
    // holds (intended, format.md §Restore), it does not pick one silently
    let d = repo();
    let a = add(
        &d,
        &[
            "add",
            "note",
            "first",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
        ],
    );
    add(
        &d,
        &[
            "add",
            "note",
            "second",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
            "--supersedes",
            &a,
        ],
    );
    let (ok, _, err) = fael(&d, &["restore", &a], "");
    assert!(ok, "{err}");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "one more",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(
        err.contains("kept all") && err.contains("--supersedes"),
        "{err}"
    );
}
