//! Two worktrees at different HEADs: each keeps its own rename cache (decision
//! 01M5CRPS). A rename that exists only on one worktree's unmerged branch must
//! not reach the other, and alternating `find` must not rewrite either file.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn git(d: &Path, args: &[&str]) {
    let s = Command::new("git").args(args).current_dir(d).status();
    assert!(s.unwrap().success(), "git {args:?}");
}

fn commit_all(d: &Path, msg: &str) {
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "-m", msg]);
}

#[test]
fn rename_on_another_worktrees_branch_stays_there() {
    let d: PathBuf = std::env::temp_dir().join(format!("fael-alias-wt-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Alias Test"],
        &["config", "user.email", "alias@example.com"],
    ] {
        git(&d, args);
    }
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    commit_all(&d, "add a");
    fael(&d, &["add", "note", "about a", "--files", "src/a.rs"]);
    fael(&d, &["find", "--files", "src/a.rs"]); // main builds its cache

    let wt = d.with_file_name(format!("{}-wt", d.file_name().unwrap().to_string_lossy()));
    git(
        &d,
        &["worktree", "add", "-q", "-b", "br", wt.to_str().unwrap()],
    );
    git(&wt, &["mv", "src/a.rs", "src/b.rs"]);
    commit_all(&wt, "rename a to b");

    let main_cache = d.join(".git/fael/cache/aliases.json");
    let before = std::fs::read(&main_cache).unwrap();
    for _ in 0..2 {
        let wt_out = fael(&wt, &["find", "--files", "src/b.rs"]);
        assert!(
            wt_out.contains("about a"),
            "wt sees its own rename: {wt_out}"
        );
        let main_out = fael(&d, &["find", "--files", "src/b.rs"]);
        assert!(
            !main_out.contains("about a"),
            "main sees wt's rename: {main_out}"
        );
    }
    assert_eq!(
        before,
        std::fs::read(&main_cache).unwrap(),
        "wt rewrote main's cache"
    );
}
