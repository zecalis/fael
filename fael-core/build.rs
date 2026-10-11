//! `FAEL_GIT`: the commit a non-release build came from (`abc1234`, `abc1234.dirty`), so two
//! dev builds of the same version tell apart (issue 01M4N4CVA). A clean checkout at a `v*`
//! tag (the release) and a build outside git (crates.io) set nothing.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let o = Command::new("git").args(args).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../fael/src");
    // moves on every commit, switch and reset, in a linked worktree too
    if let Some(head) = git(&["rev-parse", "--git-path", "logs/HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    let Some(sha) = git(&["rev-parse", "--short=7", "HEAD"]) else {
        return;
    };
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let tagged = git(&[
        "describe",
        "--tags",
        "--exact-match",
        "--match",
        "v*",
        "HEAD",
    ])
    .is_some();
    if tagged && !dirty {
        return;
    }
    let suffix = if dirty { ".dirty" } else { "" };
    println!("cargo:rustc-env=FAEL_GIT={sha}{suffix}");
}
