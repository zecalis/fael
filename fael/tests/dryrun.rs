//! Chunk 2 (PLAN-fael-selfheal-restore): one shared `evaluate()` behind heal,
//! `fael add --dry-run` and MCP `add` + `dry_run`. A dry run prints the
//! Verdict heal would act on and writes nothing — the acceptance is parity:
//! the previewed target is the row the real add supersedes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(d: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(d)
            .status()
            .unwrap()
            .success(),
        "git {args:?}"
    );
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-dryrun-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q"]);
    git(&d, &["config", "user.name", "Dry Test"]);
    git(&d, &["config", "user.email", "dry@example.com"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    for f in ["src/a.rs", "src/b.rs"] {
        std::fs::write(d.join(f), format!("// {f}\n")).unwrap();
    }
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "init"]);
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

/// Run the CLI in `dir`: (success, stdout, stderr).
fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
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
        // A rejected batch exits without reading stdin, so the write may hit
        // EPIPE; the exit status and stderr below are what the tests assert.
        let _ = c.stdin.take().unwrap().write_all(stdin.as_bytes());
    }
    let o = c.wait_with_output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// Every byte under `<repo>/.fael/log` — a dry run must leave these identical.
fn log_bytes(d: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = vec![];
    let log = d.join(".fael/log");
    if let Ok(top) = std::fs::read_dir(&log) {
        for dir in top.flatten() {
            if let Ok(files) = std::fs::read_dir(dir.path()) {
                for f in files.flatten() {
                    let p = f.path();
                    out.push((p.clone(), std::fs::read(&p).unwrap_or_default()));
                }
            }
        }
    }
    out.sort();
    out
}

/// One MCP call; returns (isError, text).
fn mcp(d: &Path, tool: &str, args: serde_json::Value) -> (bool, String) {
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": args}})
    .to_string();
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all((call + "\n").as_bytes())
        .unwrap();
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    (
        v["result"]["isError"] == true,
        v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

/// `find <id> --json` field.
fn field(d: &Path, id: &str, k: &str) -> String {
    let (ok, out, err) = fael(d, &["find", id, "--json"], "");
    assert!(ok, "{err}");
    serde_json::from_str::<serde_json::Value>(&out).unwrap()[k]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// Dry run predicts the heal, writes nothing; the real add then supersedes
/// exactly the previewed row with the previewed provenance.
#[test]
fn dry_run_predicts_heal_and_writes_nothing() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "note", "first", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    let before = log_bytes(&d);
    assert!(!before.is_empty());

    let (ok, out, err) = fael(
        &d,
        &["add", "note", "second", "--files", "src/a.rs", "--dry-run"],
        "",
    );
    assert!(ok, "{err}");
    let line = out.lines().next().unwrap_or_default();
    assert!(
        line.starts_with("dry-run FilesAct (heuristic:files) → "),
        "{line}"
    );
    let target = line.rsplit(' ').next().unwrap().to_string();
    assert!(err.contains("(open note, same branch"), "{err}");
    assert_eq!(log_bytes(&d), before, "a dry run writes nothing");

    let (ok, out, err) = fael(&d, &["add", "note", "second", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    assert!(err.contains("(open note, same branch"), "{err}");
    let filed: Vec<&str> = out.lines().next().unwrap_or_default().split(' ').collect();
    assert_eq!(field(&d, filed[0], "supersedes"), target);
    assert_eq!(field(&d, filed[0], "decision_source"), "heuristic:files");
}

/// Fresh files hold no open row: the verdict is Noop, still without writing.
#[test]
fn dry_run_noop_on_fresh_files() {
    let d = repo();
    let before = log_bytes(&d);
    let (ok, out, _) = fael(
        &d,
        &["add", "note", "fresh", "--files", "src/b.rs", "--dry-run"],
        "",
    );
    assert!(ok);
    assert_eq!(
        out.trim(),
        "dry-run Noop (none)\nwould add: note fresh → src/b.rs",
        "{out:?}"
    );
    assert_eq!(log_bytes(&d), before);
}

/// A kind the real add rejects is rejected by the dry run too — never a Noop.
#[test]
fn dry_run_rejects_unknown_kind() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &["add", "idea", "x", "--files", "src/b.rs", "--dry-run"],
        "",
    );
    assert!(!ok, "{out}");
    assert!(
        err.contains("kind must be one of decision|issue|note"),
        "{err}"
    );
}

/// `--json` prints the machine verdict: names only, evidence as enum names.
#[test]
fn dry_run_json_shape() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "note", "first", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    let (ok, out, _) = fael(
        &d,
        &[
            "add",
            "note",
            "second",
            "--files",
            "src/a.rs",
            "--dry-run",
            "--json",
        ],
        "",
    );
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["verdict"], "FilesAct");
    assert_eq!(v["source"], "heuristic:files");
    assert_eq!(v["target"], v["supersedes"]);
    assert_eq!(v["evidence"]["kind"], "Same");
    assert_eq!(v["evidence"]["named"], "No");
    assert!(v["evidence"]["files"].as_u64().unwrap() >= 1);
    assert!(!v["notes"].as_array().unwrap().is_empty());
}

/// MCP `dry_run` previews without writing; the real MCP add then acts on the
/// previewed target. Batch + preview is rejected on both surfaces.
#[test]
fn mcp_dry_run_previews_without_writing() {
    let d = repo();
    let (is_err, text) = mcp(
        &d,
        "add",
        serde_json::json!({"kind": "note", "text": "first", "files": ["src/a.rs"]}),
    );
    assert!(!is_err, "{text}");
    let before = log_bytes(&d);

    let (is_err, text) = mcp(
        &d,
        "add",
        serde_json::json!({"kind": "note", "text": "preview", "files": ["src/a.rs"], "dry_run": true}),
    );
    assert!(!is_err, "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["verdict"], "FilesAct");
    assert_eq!(v["source"], "heuristic:files");
    let target = v["target"].as_str().unwrap().to_string();
    assert_eq!(log_bytes(&d), before, "a dry run writes nothing");

    let (is_err, text) = mcp(
        &d,
        "add",
        serde_json::json!({"kind": "note", "text": "second", "files": ["src/a.rs"]}),
    );
    assert!(!is_err, "{text}");
    let filed = text.lines().next().unwrap().split(' ').nth(1).unwrap();
    assert_eq!(field(&d, filed, "supersedes"), target);

    let (is_err, text) = mcp(
        &d,
        "add",
        serde_json::json!({"rows": [{"kind": "note", "text": "x", "files": []}], "dry_run": true}),
    );
    assert!(is_err, "{text}");
    assert!(text.contains("one row"), "{text}");
}

/// `--dry-run` needs one row: the JSON-stdin batch refuses it.
#[test]
fn cli_batch_dry_run_rejected() {
    let d = repo();
    let stdin = r#"[{"kind":"note","text":"x","files":[]}]"#;
    let (ok, _, err) = fael(&d, &["add", "--json", "-", "--dry-run"], stdin);
    assert!(!ok, "batch + dry-run must fail");
    assert!(err.contains("one row"), "{err}");
    assert!(log_bytes(&d).is_empty(), "a refused batch writes nothing");
}

/// `FAEL_DIR` sends the write to a scratch dir: the repo's real `.fael/log`
/// stays byte-identical, so a wrong-cwd manual run cannot pollute it.
#[test]
fn fael_dir_isolates_the_log() {
    let d = repo();
    let scratch = d.join("scratch");
    let before = log_bytes(&d);
    let ok = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["add", "note", "scratch only", "--files", "src/a.rs"])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env("FAEL_DIR", &scratch)
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .status()
        .unwrap()
        .success();
    assert!(ok);
    assert_eq!(log_bytes(&d), before);
    assert!(scratch.join("log").exists());
}
