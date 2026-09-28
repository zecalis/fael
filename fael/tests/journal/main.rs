//! Journal + `store` (PLAN-fael-durable-log chunk 1) through the real binary:
//! a deleted branch keeps its rows (`@branch`), a failed tree write stays a
//! success off the journal, and `local` shares rows across worktrees with no
//! `.fael/` in the tree.

use std::path::{Path, PathBuf};
use std::process::Command;

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

fn git_out(d: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-{name}-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Journal Test"],
        &["config", "user.email", "journal@example.com"],
        &["commit", "-q", "--allow-empty", "-m", "init"],
    ] {
        git(&d, args);
    }
    d
}

/// `<git-common-dir>/fael/log` for `d` — `--git-common-dir` prints `.git`
/// (relative) in a plain repo, an absolute path in a worktree.
fn journal_log(d: &Path) -> PathBuf {
    let c = PathBuf::from(git_out(d, &["rev-parse", "--git-common-dir"]));
    d.join(c).join("fael/log")
}

/// The one `.jsonl` row file under `dir` (tree or journal), if any.
fn month_file(dir: &Path) -> Option<PathBuf> {
    let mut out = None;
    if let Ok(writers) = std::fs::read_dir(dir) {
        for w in writers.flatten() {
            if let Ok(files) = std::fs::read_dir(w.path()) {
                for f in files.flatten() {
                    let p = f.path();
                    if p.extension().is_some_and(|e| e == "jsonl")
                        && !p.to_string_lossy().ends_with(".close.jsonl")
                    {
                        out = Some(p);
                    }
                }
            }
        }
    }
    out
}

#[test]
fn tracked_row_survives_its_branch_deletion_tagged() {
    let d = repo("journal-tracked");
    let main = git_out(&d, &["symbolic-ref", "--short", "HEAD"]);
    git(&d, &["checkout", "-qb", "feat/journal-x"]);
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "row on a doomed branch",
            "--files",
            "doc:doomed",
        ],
    );
    assert!(ok, "{err}");
    // the row reached the journal (clone-shared) and the branch's tree
    let jf = month_file(&journal_log(&d)).expect("journal row file");
    assert!(
        std::fs::read_to_string(&jf).unwrap().contains("doomed"),
        "{jf:?}"
    );
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "doomed rows"]);
    git(&d, &["checkout", "-q", &main]);
    git(&d, &["branch", "-D", "feat/journal-x"]);

    // the tree no longer holds the row, but plain find still lists it — tagged
    assert!(month_file(&d.join(".fael/log")).is_none());
    let (ok, out, _) = fael(&d, &["find"]);
    assert!(ok, "{out}");
    assert!(out.contains("doomed"), "{out}");
    let line = out.lines().find(|l| l.contains("doomed")).unwrap();
    assert!(line.ends_with("@feat/journal-x"), "{line}");
}

#[test]
fn tracked_tree_failure_is_a_warning_not_an_error() {
    let d = repo("journal-crash");
    let (ok, _, err) = fael(&d, &["add", "note", "seed", "--files", "doc:seed"]);
    assert!(ok, "{err}");
    // block the tree month file with a directory: the tree write must fail
    let tf = month_file(&d.join(".fael/log")).expect("tree row file");
    std::fs::remove_file(&tf).unwrap();
    std::fs::create_dir_all(&tf).unwrap();

    let (ok, _, err) = fael(
        &d,
        &["add", "note", "row through a crash", "--files", "doc:crash"],
    );
    assert!(ok, "journal commit must still succeed: {err}");
    assert!(err.contains("tree write failed"), "{err}");
    assert!(err.contains("do not retry"), "{err}");

    // the journal-only row lists (untagged: stamped branch is this branch)
    let (ok, out, _) = fael(&d, &["find"]);
    assert!(ok, "{out}");
    assert!(out.contains("row through a crash"), "{out}");
    let line = out
        .lines()
        .find(|l| l.contains("row through a crash"))
        .unwrap();
    assert!(!line.contains('@'), "{line}");

    // healed (the same bytes reach the tree later): the union lists the id once
    std::fs::remove_dir(&tf).unwrap();
    let jf = month_file(&journal_log(&d)).expect("journal row file");
    std::fs::write(&tf, std::fs::read(&jf).unwrap()).unwrap();
    let (ok, out, _) = fael(&d, &["find", "--json", "--all"]);
    assert!(ok, "{out}");
    let mut ids: Vec<String> = out
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["id"].as_str().map(String::from))
        .collect();
    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "union must not double a healed row");
}

#[test]
fn local_shares_rows_across_worktrees_with_no_tree_log() {
    let d = repo("journal-local");
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
    let wt = d.with_extension("wt2");
    git(&d, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    let (ok, _, err) = fael(&d, &["add", "note", "local row", "--files", "doc:local"]);
    assert!(ok, "{err}");

    // no log in the tree — the row lives in the clone-shared journal only
    assert!(
        !d.join(".fael/log").exists(),
        "local mode must not write the tree log"
    );
    // the sibling worktree (no .fael/ at all) reads the same row
    assert!(!wt.join(".fael").exists());
    let (ok, out, _) = fael(&wt, &["find"]);
    assert!(ok, "{out}");
    assert!(out.contains("local row"), "{out}");

    git(&d, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
}

#[test]
fn unknown_store_is_rejected() {
    let d = repo("journal-store-bad");
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"cloud\"\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "x", "--files", "doc:x"]);
    assert!(!ok);
    assert!(err.contains("tracked") && err.contains("local"), "{err}");
}

fn past_row(id: &str, text: &str, file: &str) -> String {
    format!(
        r#"{{"v":1,"id":"{id}","ts":"2000-01-01T00:00:00.000Z","by":"test-user-","kind":"note","text":"{text}","files":["{file}"]}}"#
    )
}

fn close_row(id: &str, target: &str) -> String {
    format!(
        r#"{{"v":1,"id":"{id}","ts":"2000-01-02T00:00:00.000Z","by":"test-user-","ref":"{target}","text":"done"}}"#
    )
}

/// compact must rewrite the journal too, reading both roots as one log:
/// a `--prune`d row cannot resurface through the union, and a close that
/// lives only in the journal still folds (01M3HQHJ2).
#[test]
fn compact_rewrites_both_roots_without_resurrection() {
    let d = repo("journal-compact");
    std::fs::write(d.join("here.rs"), "x").unwrap();
    let rows = format!(
        "{}\n{}\n{}\n",
        past_row("A0000000000000000000000001", "pruned gone", "gone.rs"),
        past_row("A0000000000000000000000002", "kept closed", "here.rs"),
        past_row("A0000000000000000000000003", "kept open", "here.rs"),
    );
    let r1 = close_row("C0000000000000000000000001", "A0000000000000000000000001");
    let r2 = close_row("C0000000000000000000000002", "A0000000000000000000000002");
    let tree = d.join(".fael/log/test-user-");
    let jrnl = journal_log(&d).join("test-user-");
    for dir in [&tree, &jrnl] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("2000-01.jsonl"), &rows).unwrap();
    }
    std::fs::write(tree.join("2000-01.close.jsonl"), format!("{r1}\n")).unwrap();
    // the close for the kept row lives only in the journal (a failed tree write)
    std::fs::write(jrnl.join("2000-01.close.jsonl"), format!("{r1}\n{r2}\n")).unwrap();

    let (ok, out, err) = fael(&d, &["compact", "--prune"]);
    assert!(ok, "{err} {out}");
    // past-month sources are gone from both roots
    assert!(!tree.join("2000-01.jsonl").exists());
    assert!(!jrnl.join("2000-01.jsonl").exists());
    // the pruned row stays gone — the journal does not resurrect it
    let (_, all, _) = fael(&d, &["find", "--all"]);
    assert!(!all.contains("pruned gone"), "{all}");
    // the journal-only close folded: hidden by default, listed with --all
    let (_, open, _) = fael(&d, &["find"]);
    assert!(!open.contains("kept closed"), "{open}");
    assert!(open.contains("kept open"), "{open}");
    assert!(all.contains("kept closed"), "{all}");
}

/// An import lands in the journal first, so a deleted branch cannot take it.
#[test]
fn imported_rows_survive_branch_deletion() {
    let d = repo("journal-import");
    let main = git_out(&d, &["symbolic-ref", "--short", "HEAD"]);
    git(&d, &["checkout", "-qb", "feat/import-x"]);
    let src = d.join("old-memory");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("log.jsonl"),
        r#"{"ts":"2026-01-01T00:00:00Z","agent":"delamind","id":"muft0001","kind":"decision","text":"imported survives","files":["src/a.rs"],"v":2}"#
            .to_string()
            + "\n",
    )
    .unwrap();
    let (ok, _, err) = fael(&d, &["import", src.to_str().unwrap()]);
    assert!(ok, "{err}");
    let jimp = journal_log(&d).join("_import");
    let has = std::fs::read_dir(&jimp)
        .map(|rd| {
            rd.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        })
        .unwrap_or(false);
    assert!(has, "import must reach the journal: {jimp:?}");
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "imported"]);
    git(&d, &["checkout", "-q", &main]);
    git(&d, &["branch", "-D", "feat/import-x"]);

    // the tree no longer holds it; the journal does
    let (ok, out, _) = fael(&d, &["find"]);
    assert!(ok, "{out}");
    assert!(out.contains("imported survives"), "{out}");
}
