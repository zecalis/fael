//! `fael chunk status <uid>` (f6): both sides of a joint run at once —
//! branch/base/ahead per side plus unstaged. DB from the store, git live in
//! the CLI; read-only, no state change.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(crate::common::fael_bin())
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

fn git(d: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(d)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-status-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/x.rs"), "one\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "init"]);
    let (ok, _, err) = fael(&d, &["plan", "new", "pl", "PL"]);
    assert!(ok, "{err}");
    d
}

fn chunk(d: &Path, title: &str) -> String {
    let (ok, uid, err) = fael(
        d,
        &[
            "chunk", "add", title, "--plan", "pl", "--brief", "b", "--scope", "src/x.rs",
        ],
    );
    assert!(ok, "{err}");
    uid.trim().to_string()
}

/// A pair's status names both sides, each with branch/base/ahead + unstaged.
#[test]
fn pair_status_shows_both_sides_with_git_reads() {
    let d = repo();
    let a = chunk(&d, "a2 work");
    let b = chunk(&d, "a4 work");
    let (ok, _, err) = fael(&d, &["chunk", "start", &a]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["chunk", "start", &b, "--with", &a, "--as", "pair"]);
    assert!(ok, "{err}");
    std::fs::write(d.join("src/x.rs"), "one\ntwo\n").unwrap();
    git(&d, &["add", "src/x.rs"]);
    git(&d, &["commit", "-qm", "a2: touch x"]);
    std::fs::write(d.join("src/x.rs"), "one\ntwo\nunstaged\n").unwrap();

    let (ok, out, err) = fael(&d, &["chunk", "status", &a]);
    assert!(ok, "{err}");
    for needle in [
        &a,
        &b,
        "branch",
        "base",
        "ahead 1",
        "a2: touch x",
        "M src/x.rs",
    ] {
        assert!(out.contains(needle), "{needle} missing in:\n{out}");
    }
    // either side answers the same joint run
    let (ok, out_b, err) = fael(&d, &["chunk", "status", &b]);
    assert!(ok, "{err}");
    assert!(out_b.contains(&a) && out_b.contains(&b), "{out_b}");

    // --json carries both uids with their git reads
    let (ok, out, err) = fael(&d, &["chunk", "status", &a, "--json"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("status --json");
    let uids: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["uid"].as_str().unwrap())
        .collect();
    assert!(
        uids.contains(&a.as_str()) && uids.contains(&b.as_str()),
        "{uids:?}"
    );
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .all(|s| s["ahead"].as_array().is_some_and(|a| a.len() == 1)),
        "{v}"
    );

    // read-only: both chunks still run
    let (ok, out, err) = fael(&d, &["chunk", "show", &a]);
    assert!(ok, "{err}");
    assert!(out.contains("running"), "{out}");
}

/// A solo chunk is one side; an unknown uid rejects like show does.
#[test]
fn solo_status_is_one_side_and_unknown_rejects() {
    let d = repo();
    let c = chunk(&d, "solo");
    let (ok, out, err) = fael(&d, &["chunk", "status", &c]);
    assert!(ok, "{err}");
    assert!(
        out.contains("status 1 side(s)") && out.contains(&c),
        "{out}"
    );
    let (ok, _, err) = fael(&d, &["chunk", "status", "01M6ZZZZZZZZZZZZZZZZZZZZZZ"]);
    assert!(!ok, "unknown uid accepted");
    assert!(err.contains("no chunk"), "{err}");
}
