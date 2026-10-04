//! doctor [Drifted] through the real binary: an open row whose file took 10+
//! commits after it was written is listed; a row on an untouched file is not,
//! and a bare `fael bump` restarts the count.

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(bin: &str, dir: &Path, args: &[&str]) -> (bool, String) {
    let o = Command::new(bin)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let s = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.success(), s)
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String) {
    run(env!("CARGO_BIN_EXE_fael"), dir, args)
}

fn git(dir: &Path, args: &[&str]) {
    let (ok, out) = run("git", dir, args);
    assert!(ok, "git {args:?}: {out}");
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-drift-cli-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q"]);
    git(&d, &["config", "user.name", "Test User"]);
    git(&d, &["config", "user.email", "t@example.com"]);
    d
}

#[test]
fn doctor_lists_rows_whose_files_moved_on() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    std::fs::write(d.join("src/b.rs"), "").unwrap();
    let (ok, out) = fael(
        &d,
        &[
            "add", "decision", "a holds", "--key", "t:a", "--files", "src/a.rs",
        ],
    );
    assert!(ok, "{out}");
    let (ok, out) = fael(
        &d,
        &[
            "add", "decision", "b holds", "--key", "t:b", "--files", "src/b.rs",
        ],
    );
    assert!(ok, "{out}");
    let (_, out) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Drifted]"), "{out}");
    for i in 0..10 {
        std::fs::write(d.join("src/a.rs"), format!("// {i}\n")).unwrap();
        git(&d, &["add", "src/a.rs"]);
        git(&d, &["commit", "-q", "-m", &format!("a {i}")]);
    }
    // two rows born in one second share an id prefix: match by text
    let (_, out) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Drifted]: 1 open row(s)")
            && out.contains("×10 a holds")
            && !out.contains("b holds"),
        "{out}"
    );
    // checked and still true: a bare bump restarts the count on the same id
    // (past the last commit's second — drift counts commits at or after it)
    let (_, out) = fael(&d, &["find", "a holds", "--json"]);
    let row: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    let id = row["id"].as_str().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let (ok, out) = fael(&d, &["bump", id]);
    assert!(ok && out.starts_with(id), "{out}");
    let (_, out) = fael(&d, &["doctor"]);
    assert!(!out.contains("[Drifted]"), "{out}");
}
