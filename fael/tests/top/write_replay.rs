//! c1 lifecycle replay (plan fael-secretary): replay add+close+supersede twice
//! on the same files returns the same ids and creates zero new rows.

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
    let d = std::env::temp_dir().join(format!("fael-write-replay-{}", fael_core::ulid()));
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

fn id_of(out: &str) -> &str {
    out.split_whitespace().next().unwrap()
}

fn rows(d: &Path) -> usize {
    let (_, out, _) = fael(d, &["find", "--kind", "issue", "--json", "--all"]);
    out.lines().filter(|l| l.starts_with('{')).count()
}

fn closes(d: &Path) -> usize {
    let (_, out, _) = fael(d, &["find", "--kind", "issue", "--json", "--all"]);
    out.matches("\"ref\":").count()
}

#[test]
fn write_replay_add_close_supersede_twice_same_ids_zero_rows() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// x\n").unwrap();
    // add A
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "replay hot path",
            "--title",
            "replay",
            "--files",
            "src/a.rs",
            "--key",
            "replay:hot",
        ],
    );
    assert!(ok, "{err}");
    let a = id_of(&out).to_string();
    // replay the same add: same id, zero new rows
    let before = rows(&d);
    let (ok, out2, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "replay hot path",
            "--title",
            "other",
            "--files",
            "src/a.rs",
            "--key",
            "replay:hot",
        ],
    );
    assert!(ok, "{err}");
    assert_eq!(
        id_of(&out2),
        a.as_str(),
        "duplicate add returns the same id"
    );
    assert!(err.contains("already open"), "{err}");
    assert_eq!(rows(&d), before, "duplicate add writes zero rows");
    // supersede A -> B
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "replay hot path v2",
            "--title",
            "replay v2",
            "--files",
            "src/a.rs",
            "--key",
            "replay:hot2",
            "--supersedes",
            &a,
        ],
    );
    assert!(ok, "{err}");
    let b = id_of(&out).to_string();
    // replay the supersede with identical content: same survivor, zero rows
    let before = rows(&d);
    let (ok, out2, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "replay hot path v2",
            "--title",
            "replay v2",
            "--files",
            "src/a.rs",
            "--key",
            "replay:hot2",
            "--supersedes",
            &a,
        ],
    );
    assert!(ok, "{err}");
    assert_eq!(
        id_of(&out2),
        b.as_str(),
        "supersede retry returns the survivor"
    );
    assert!(err.contains("not filed again"), "{err}");
    assert_eq!(rows(&d), before, "supersede retry writes zero rows");
    // close B (the head)
    let (ok, out, err) = fael(&d, &["close", &b, "broke → fixed; guard `src/a_test.rs`"]);
    assert!(ok, "{err}");
    let c = id_of(&out).to_string();
    // replay the close: same close id, zero new close rows
    let before_c = closes(&d);
    let (ok, out2, err) = fael(&d, &["close", &b, "broke → fixed; guard `src/a_test.rs`"]);
    assert!(ok, "{err}");
    assert_eq!(
        id_of(&out2),
        c.as_str(),
        "close retry returns the same close id"
    );
    assert!(err.contains("not filed again"), "{err}");
    assert_eq!(closes(&d), before_c, "close retry writes zero rows");
}
