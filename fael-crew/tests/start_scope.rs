//! plan fael-row-retire c3: `chunk start` prints the open decisions and notes
//! on files under the chunk's scope, verdicts ready; a clean scope prints
//! nothing extra.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{args:?}: {err}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    let ok = Command::new("git")
        .args(id)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn init() -> (PathBuf, PathBuf) {
    let tmp = std::env::temp_dir().join(format!("fael-start-scope-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("README"), "base").unwrap();
    git(&d, &["add", "README"]);
    git(&d, &["commit", "-qm", "init"]);
    (d, state)
}

/// The short id `out` names `id` by — fael abbreviates, and no other row's
/// short id is a prefix of this one's. `None` when `out` never names it.
fn said_id(out: &str, id: &str) -> Option<String> {
    out.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|w| w.len() >= 8 && id.starts_with(w))
        .map(String::from)
}

#[test]
fn start_lists_the_open_rows_under_its_scope() {
    let (d, state) = init();
    let f = |args: &[&str]| fael(&d, &state, args);
    std::fs::create_dir_all(d.join("w")).unwrap();
    for p in ["w/a.txt", "w/b.txt"] {
        std::fs::write(d.join(p), "v1\n").unwrap();
    }
    git(&d, &["add", "w"]);
    git(&d, &["commit", "-qm", "w"]);
    // two files, so touching one leaves it live, not dormant (01M5528X)
    let out = f(&[
        "add",
        "decision",
        "held rule on w",
        "--files",
        "w/a.txt,w/b.txt",
        "--force",
    ]);
    let id = out
        .split_whitespace()
        .find(|w| w.starts_with("01"))
        .unwrap()
        .trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string();
    std::fs::write(d.join("w/a.txt"), "v2\n").unwrap();

    // a dir scope covers the file under it on a `/` boundary
    let u = f(&["chunk", "add", "u", "--brief", "b", "--scope", "w/"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let out = f(&["chunk", "start", &u]);
    let short = said_id(&out, &id).unwrap_or_default();
    for want in [
        format!("fael bump {short}"),
        format!("fael close {short} \"now in `w/a.txt`\""),
        format!("--supersedes {short}"),
    ] {
        assert!(out.contains(&want), "{want}: {out}");
    }
}

#[test]
fn start_on_a_clean_scope_prints_nothing_extra() {
    let (d, state) = init();
    let f = |args: &[&str]| fael(&d, &state, args);
    std::fs::create_dir_all(d.join("w")).unwrap();
    for p in ["w/a.txt", "w/b.txt"] {
        std::fs::write(d.join(p), "v1\n").unwrap();
    }
    git(&d, &["add", "w"]);
    git(&d, &["commit", "-qm", "w"]);
    f(&[
        "add",
        "decision",
        "held rule on w",
        "--files",
        "w/a.txt,w/b.txt",
        "--force",
    ]);
    std::fs::write(d.join("w/a.txt"), "v2\n").unwrap();

    let v = f(&["chunk", "add", "v", "--brief", "b", "--scope", "other/"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let out = f(&["chunk", "start", &v]);
    assert!(!out.contains("fael bump"), "{out}");
    assert!(!out.contains("fael close"), "{out}");
    assert!(!out.contains("--supersedes"), "{out}");
}
