//! f1 CLI side: `find --recall wide` unions the exact file hits, the rows
//! sharing a key with one, and the text match off any file — in that order.
//! The default stays today's exact hit; any other value exits 1.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", root.join("state"))
        // x and y share a key: past the 1 s same-burst window (a slow runner
        // between two spawned adds) y would supersede x and hide it. Pin the
        // window open so the setup never depends on the clock.
        .env("FAEL_BURST_MS", "3600000")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-find-recall-{}", fael_core::ulid()));
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

fn add(d: &Path, text: &str, file: &str, key: Option<&str>) {
    std::fs::write(d.join(file), "// x\n").unwrap();
    let mut args = vec!["add", "note", text, "--files", file];
    if let Some(k) = key {
        args.extend(["--key", k]);
    }
    let (ok, _, err) = fael(d, &args);
    assert!(ok, "{err}");
}

#[test]
fn wide_unions_key_sharing_and_unfiled_text() {
    let d = repo();
    add(&d, "sunset deploy on x", "src/x.rs", Some("k"));
    add(&d, "unrelated bookkeeping", "src/y.rs", Some("k"));
    add(&d, "sunset deploy on z", "src/z.rs", None);
    add(&d, "unrelated other", "src/w.rs", None);
    let q = ["find", "sunset deploy", "--files", "src/x.rs"];
    // default: only the exact hit
    let (ok, narrow, err) = fael(&d, &q);
    assert!(ok && narrow.contains("sunset deploy on x"), "{err}{narrow}");
    assert!(
        !narrow.contains("bookkeeping")
            && !narrow.contains("sunset deploy on z")
            && !narrow.contains("unrelated other"),
        "{narrow}"
    );
    // wide: exact, key-sharing, unfiled text — in that order, never the unrelated row
    let (ok, wide, err) = fael(&d, &[&q[..], &["--recall", "wide"]].concat());
    assert!(ok, "{err}");
    let at = |s: &str| wide.find(s).unwrap_or_else(|| panic!("{s}: {wide}"));
    let (a, b, c) = (
        at("sunset deploy on x"),
        at("unrelated bookkeeping"),
        at("sunset deploy on z"),
    );
    assert!(a < b && b < c, "{wide}");
    assert!(!wide.contains("unrelated other"), "{wide}");
}

#[test]
fn an_unknown_recall_value_exits_1() {
    let d = repo();
    add(&d, "a row", "src/x.rs", None);
    let (ok, _, err) = fael(&d, &["find", "a row", "--recall", "broad"]);
    assert!(!ok && err.contains("recall"), "{err}");
}
