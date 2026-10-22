//! Open-duplicate guard on `fael add`: an exact repeat of an open row writes
//! nothing and names the old id; a same-file, similar-title issue gets a hint.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", root.join("state"))
        .env("FAEL_BURST_MS", "3600000")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-add-dup-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Test User"],
        &["config", "user.email", "t@example.com"],
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
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    d
}

fn add(d: &Path, kind: &str, text: &str, title: &str, file: &str) -> (bool, String, String) {
    std::fs::write(d.join(file), "// x\n").unwrap();
    fael(d, &["add", kind, text, "--title", title, "--files", file])
}

fn count(d: &Path) -> usize {
    let (_, out, _) = fael(d, &["find", "--kind", "issue", "--json"]);
    out.lines().filter(|l| l.starts_with('{')).count()
}

#[test]
fn exact_repeat_writes_nothing_and_names_the_old_id() {
    let d = repo();
    let (ok, id1, _) = add(
        &d,
        "issue",
        "Parser drops the last line",
        "parser drops last line",
        "src/a.rs",
    );
    assert!(ok);
    let (ok, id2, err) = add(
        &d,
        "issue",
        "parser  drops the\nlast LINE ",
        "other title",
        "src/a.rs",
    );
    assert!(ok, "{err}");
    assert_eq!(id1.split_whitespace().next(), id2.split_whitespace().next());
    assert!(
        err.contains("already open:") && err.contains("not filed again"),
        "{err}"
    );
    assert_eq!(count(&d), 1);
}

#[test]
fn same_text_other_files_is_written() {
    let d = repo();
    add(
        &d,
        "issue",
        "Parser drops the last line",
        "parser drops last line",
        "src/a.rs",
    );
    let (ok, _, err) = add(
        &d,
        "issue",
        "Parser drops the last line",
        "parser drops last line",
        "src/b.rs",
    );
    assert!(ok && !err.contains("already open"), "{err}");
    assert_eq!(count(&d), 2);
}

#[test]
fn near_title_on_a_shared_file_hints_and_unrelated_stays_silent() {
    let d = repo();
    let (_, id1, _) = add(
        &d,
        "issue",
        "first body",
        "parser drops the last line",
        "src/a.rs",
    );
    let (ok, _, err) = add(
        &d,
        "issue",
        "second body",
        "parser drops last line again",
        "src/a.rs",
    );
    assert!(ok);
    let old = &id1.split_whitespace().next().unwrap()[..8];
    assert!(
        err.contains("open on these files:") && err.contains(old),
        "{err}"
    );
    let (_, _, err) = add(
        &d,
        "issue",
        "third body",
        "colour theme flickers on resize",
        "src/a.rs",
    );
    assert!(!err.contains("open on these files"), "{err}");
}
