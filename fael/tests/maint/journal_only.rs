//! A `store = "local"` repo keeps its rows in the clone's journal, none in the
//! tree: `doctor` and rename following must read them there, not report "never
//! adopted".

use super::{fael, repo};
use std::path::Path;

fn git(d: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(args)
        .current_dir(d)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A repo whose only rows are in the journal: one row filed on `src/a.rs`.
fn local_repo() -> (std::path::PathBuf, String) {
    let d = repo();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "a"]);
    let (ok, out, err) = fael(&d, &["add", "note", "choice", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    assert!(!d.join(".fael/log").exists(), "the tree holds no log");
    (d, out.split_whitespace().next().unwrap().to_string())
}

#[test]
fn doctor_reads_the_journal() {
    let (d, _) = local_repo();
    let (ok, out, err) = fael(&d, &["doctor"]);
    assert!(ok, "{out}{err}");
    assert!(!out.contains("never adopted"), "{out}");
    // nothing here lives in git: no merge=union / gitignore errors
    assert!(!out.contains("merge=union"), "{out}");
}

#[test]
fn doctor_fix_repairs_a_broken_journal_line() {
    let (d, _) = local_repo();
    let journal = d.join(".git/fael/log");
    let file = walk(&journal);
    let mut s = std::fs::read_to_string(&file).unwrap();
    s.push_str("{not json}\n");
    std::fs::write(&file, s).unwrap();
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(!ok, "a broken line is an error: {out}");
    let (ok, out, err) = fael(&d, &["doctor", "--fix"]);
    assert!(ok, "{out}{err}");
    assert!(fael(&d, &["doctor"]).0);
}

#[test]
fn rename_following_reads_the_journal() {
    let (d, id) = local_repo();
    git(&d, &["mv", "src/a.rs", "src/b.rs"]);
    git(&d, &["commit", "-q", "-am", "a to b"]);
    let (ok, out, err) = fael(&d, &["find", "--files", "src/b.rs"]);
    assert!(ok, "{err}");
    assert!(out.contains(&id[..8]), "{out}");
    // the cache sits in the journal: the tree still has no cache of its own
    assert!(d.join(".git/fael/cache/aliases.json").is_file());
    assert!(!d.join(".fael/cache").exists());
}

fn walk(dir: &Path) -> std::path::PathBuf {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            return walk(&p);
        }
        if p.extension().is_some_and(|x| x == "jsonl") {
            return p;
        }
    }
    panic!("no jsonl under {}", dir.display());
}
