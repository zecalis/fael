//! An unset `store`: `local` for a repo with no tree log, `tracked` where one
//! already sits — and a `local` repo keeps every feature without a `.fael/`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join(".git/state"))
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(d)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-store-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    git(&d, &["init", "-q"]);
    git(&d, &["config", "user.name", "Store Test"]);
    git(&d, &["config", "user.email", "store@example.com"]);
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "init"]);
    d.canonicalize().unwrap()
}

fn add(d: &Path, text: &str) -> String {
    let (ok, out, err) = fael(d, &["add", "note", text, "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

#[test]
fn a_fresh_repo_keeps_rows_out_of_the_tree() {
    let d = repo();
    let id = add(&d, "fresh row");
    assert!(!d.join(".fael").exists(), "no .fael/ in the tree");
    assert!(
        d.join(".git/fael/log").is_dir(),
        "the row is in the journal"
    );
    let (_, out, _) = fael(&d, &["find", "--files", "src/a.rs"]);
    assert!(out.contains(&id[..8]), "{out}");

    // doctor: no NoLog/Union complaints, one note on where rows live
    let (ok, out, err) = fael(&d, &["doctor"]);
    assert!(ok, "{out}{err}");
    assert!(
        out.contains("[Local]") && out.contains("fael.remote"),
        "{out}"
    );
    assert!(
        !out.contains("[NoLog]") && !out.contains("[Union]"),
        "{out}"
    );
    git(&d, &["config", "fael.remote", "/nowhere.git"]);
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(!out.contains("fael.remote"), "remote set, no hint: {out}");
}

#[test]
fn a_repo_with_a_tree_log_stays_tracked() {
    let d = repo();
    std::fs::create_dir_all(d.join(".fael/log")).unwrap();
    add(&d, "tree row");
    let n = std::fs::read_dir(d.join(".fael/log")).unwrap().count();
    assert_eq!(n, 1, "the row landed in the tree log");
}

#[test]
fn a_local_repo_follows_renames_without_a_tree_cache() {
    let d = repo();
    let id = add(&d, "before the move");
    git(&d, &["mv", "src/a.rs", "src/b.rs"]);
    git(&d, &["commit", "-qm", "move"]);
    let (_, out, _) = fael(&d, &["find", "--files", "src/b.rs"]);
    assert!(out.contains(&id[..8]), "{out}");
    assert!(!d.join(".fael").exists(), "cache stays in the git dir");
    assert!(d.join(".git/fael/cache/aliases.json").is_file());
}
