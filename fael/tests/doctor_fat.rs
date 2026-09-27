//! Chunk 10 through the real binary: doctor [Fat] repeats the add-time
//! warnings the agent skipped — open rows only, closed rows never count.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-fat-cli-{}", fael_core::ulid()));
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
    d
}

#[test]
fn doctor_flags_fat_rows_and_ignores_closed_ones() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    // the plan's done criterion: open decision, no key, three separators
    let (ok, out, err) = fael(
        &d,
        &["add", "decision", "a; b; c; d", "--files", "src/a.rs"],
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, _) = fael(&d, &["doctor", "--fix"]);
    assert!(ok);
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Fat]: 1 open row(s)") && out.contains(&id[..8]),
        "{out}"
    );
    // a keyed single-topic decision next to it stays silent
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "single topic",
            "--key",
            "a:b",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let (_, out, _) = fael(&d, &["doctor"]);
    assert!(
        out.contains("note [Fat]: 1 open row(s)") && out.contains(&id[..8]),
        "{out}"
    );
    // closing the fat row clears it — closed rows never count
    let (ok, _, err) = fael(&d, &["close", &id[..12], "split done"]);
    assert!(ok, "{err}");
    let (ok, out, _) = fael(&d, &["doctor"]);
    assert!(ok && !out.contains("[Fat]"), "{out}");
}
