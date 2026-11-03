//! `fael gate-broken`: a gate failure that reproduces on main is filed once as
//! an issue keyed `gate:<check>` — a second call reports the open row and
//! writes nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("../state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let base = std::env::temp_dir().join(format!("fael-gate-broken-{}", fael_core::ulid()));
    let d = base.join("r");
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
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    d
}

fn open_count(d: &Path, key: &str) -> usize {
    let (_, out, _) = fael(d, &["find", "--json", "--key", key]);
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .count()
}

#[test]
fn a_broken_main_is_filed_once_per_check() {
    let d = repo();
    let first = [
        "gate-broken",
        "--check",
        "clippy",
        "--files",
        "src/a.rs",
        "--on",
        "abc123",
        "--detail",
        "agent-parity.rs is 412 lines, limit 400",
    ];
    let (ok, out, err) = fael(&d, &first);
    assert!(ok, "{err}");
    assert!(out.contains("→"), "{out}");
    assert_eq!(open_count(&d, "gate:clippy"), 1);
    let body = fael(&d, &["find", "--full", "gate:clippy"]).1;
    assert!(body.contains("abc123"), "{body}");
    // a second call reports the open row and writes nothing
    let second = [
        "gate-broken",
        "--check",
        "clippy",
        "--files",
        "src/a.rs",
        "--on",
        "abc123",
        "--detail",
        "same failure again",
    ];
    let (ok, out, err) = fael(&d, &second);
    assert!(ok, "{err}");
    assert!(out.contains("already has an open issue"), "{out}");
    assert_eq!(open_count(&d, "gate:clippy"), 1);
    // closed, the next breakage files again
    let (_, out, _) = fael(&d, &["find", "--json", "--key", "gate:clippy"]);
    let id: String = serde_json::from_str::<serde_json::Value>(out.lines().next().unwrap())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        fael(
            &d,
            &[
                "close",
                &id,
                "old lint fails main → disabled it; guard `src/a.rs`"
            ]
        )
        .0
    );
    let third = [
        "gate-broken",
        "--check",
        "clippy",
        "--files",
        "src/a.rs",
        "--on",
        "abc123",
        "--detail",
        "broke again",
    ];
    let (ok, _, err) = fael(&d, &third);
    assert!(ok, "{err}");
    assert_eq!(open_count(&d, "gate:clippy"), 1);
}

#[test]
fn missing_or_bad_inputs_are_rejected() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["gate-broken", "--files", "src/a.rs", "--on", "abc123"],
    );
    assert!(!ok && err.contains("--check needs a gate name"), "{err}");
    let (ok, _, err) = fael(&d, &["gate-broken", "--check", "clippy", "--on", "abc123"]);
    assert!(!ok && err.contains("--files is required"), "{err}");
    let (ok, _, err) = fael(
        &d,
        &[
            "gate-broken",
            "--check",
            "Clippy",
            "--files",
            "src/a.rs",
            "--on",
            "abc123",
        ],
    );
    assert!(!ok && err.contains("lowercase"), "{err}");
}
