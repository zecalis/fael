//! Chunk 6b (batch `add --json -`), 6c (the block's command runs verbatim)
//! and 6f (English rows warn, never reject) — the real binary in a throwaway
//! git repo, like cli.rs.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Per-child `FAEL_STATE_DIR` at `<repo root>/state`, so a real session on this
/// machine never leaks in and tests run in parallel without a global env lock.
fn state_env(c: &mut Command, dir: &Path) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    c.env("FAEL_STATE_DIR", root.join("state"));
}

fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(dir);
    state_env(&mut c, dir);
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
    let d = std::env::temp_dir().join(format!("fael-batch-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Batch Test"],
        &["config", "user.email", "batch@example.com"],
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
    for f in ["a.rs", "b.rs", "c.rs"] {
        std::fs::write(d.join("src").join(f), "// x\n").unwrap();
    }
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

fn texts(d: &Path) -> String {
    let mut all = String::new();
    for dir in std::fs::read_dir(d.join(".fael/log"))
        .into_iter()
        .flatten()
        .flatten()
    {
        for f in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            all += &std::fs::read_to_string(f.path()).unwrap_or_default();
        }
    }
    all
}

/// Chunk 6b: two good rows + one bad kind in one stdin array — the bad row
/// reports alone (`rejected: row 1:`), the rest save, exit is failure.
#[test]
fn batch_partial_save_names_the_bad_row() {
    let d = repo();
    let stdin = serde_json::json!([
        {"kind": "note", "text": "first batch row", "files": ["src/a.rs"]},
        {"kind": "nope", "text": "bad kind row", "files": ["src/b.rs"]},
        {"kind": "issue", "text": "third batch row", "files": ["src/c.rs"]},
    ])
    .to_string();
    let (ok, out, _) = fael(&d, &["add", "--json", "-"], &stdin);
    assert!(!ok, "{out}");
    // batch rides `--json`: one JSON row per line, like single add
    let rows: Vec<&str> = out.lines().filter(|l| l.starts_with('{')).collect();
    assert_eq!(rows.len(), 2, "{out}");
    assert!(out.contains("rejected: row 1:"), "{out}");
    let log = texts(&d);
    assert!(
        log.contains("first batch row") && log.contains("third batch row"),
        "{log}"
    );
    assert!(!log.contains("bad kind row"), "{log}");
}

/// Chunk 6b: stdin that is not an array rejects the whole call, saving nothing.
#[test]
fn batch_non_array_rejects() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "--json", "-"], r#"{"kind":"note"}"#);
    assert!(!ok && err.contains("not a JSON array"), "{err}");
}

/// Chunk 6c: the command the stop block prints runs verbatim — files
/// prefilled, no `<files>` placeholder left to fill in.
#[test]
fn block_command_runs_verbatim() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "login breaks on retry",
            "--files",
            "src/a.rs,src/b.rs,src/c.rs",
        ],
        "",
    );
    assert!(ok, "{out} {err}");
    assert!(out.contains("→"), "{out}");
}

/// Chunk 6f: a Thai title files with exit 0 + exactly one warning line —
/// never a reject. Symbols (→, ≤) and accented Latin (é) pass silently.
#[test]
fn thai_title_warns_once_and_files() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "stale notes after merge",
            "--title",
            "หัวข้อไทย",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    let warns: Vec<&str> = err
        .lines()
        .filter(|l| l.contains("row not in English"))
        .collect();
    assert_eq!(
        warns,
        [
            "row not in English — write rows in English from now on; cite a foreign term in `backticks`"
        ],
        "{err}"
    );
    assert!(texts(&d).contains("stale notes after merge"));
}

#[test]
fn symbols_and_accents_pass_silently() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "a → b when ≤ 3, café laté",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("not in English"), "{err}");
}
