//! Lane/pair seams found by hand (issues 01M6E23QPV, 01M6E23QTD, 01M6E23QX4): one git
//! worktree per running chunk, every call made from its own worktree.

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

/// A repo with one commit and the worktrees `names`; returns them in order.
fn worktrees(names: &[&str]) -> Vec<PathBuf> {
    let d = std::env::temp_dir().join(format!("fael-joint-{}", fael_core::ulid()));
    let main = d.join("main");
    std::fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]);
    names
        .iter()
        .map(|n| {
            let w = d.join(n);
            // git on Windows rejects the `\\?\` verbatim prefix `canonicalize` adds.
            let arg = w.to_string_lossy().replacen(r"\\?\", "", 1);
            git(&main, &["worktree", "add", "-q", "-b", n, &arg]);
            w.canonicalize().unwrap()
        })
        .collect()
}

/// A chunk whose scope is `src/x.rs`.
fn chunk(w: &Path, title: &str) -> String {
    let (ok, uid, err) = fael(
        w,
        &["chunk", "add", title, "--brief", "b", "--scope", "src/x.rs"],
    );
    assert!(ok, "{err}");
    uid.trim().to_string()
}

fn ok(w: &Path, args: &[&str]) -> String {
    let (good, out, err) = fael(w, args);
    assert!(good, "{args:?}: {out}{err}");
    out
}

/// Issue 01M6E23QPV: `touch --msg` reached the arg parser as an unknown flag.
#[test]
fn touch_msg_shows_as_doing_on_the_other_cards() {
    let w = worktrees(&["wa", "wb"]);
    let (a, b) = (chunk(&w[0], "A"), chunk(&w[0], "B"));
    ok(&w[0], &["chunk", "start", &a]);
    ok(
        &w[0],
        &[
            "chunk",
            "touch",
            &a,
            "src/x.rs",
            "--msg",
            "rewriting parse()",
        ],
    );
    let shown = ok(&w[1], &["chunk", "show", &b]);
    assert!(shown.contains("doing: rewriting parse()"), "{shown}");
}
