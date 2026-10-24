//! Shared by the integration tests: these drive the `fael` binary, which hands the crew verbs to
//! `fael-crew`. Cargo builds only this package's own binary for a test, so `fael` is looked up
//! next to it — present after `cargo test --workspace`.

use std::path::PathBuf;
use std::process::Command;

/// The `fael` binary built beside `fael-crew`, with a freshness guard.
/// A partial build (`cargo test -p fael-crew`) leaves a stale `target/debug/fael`
/// whose `plan --help` prints the old verbs while `fael-crew` is fresh —
/// the dispatch mismatch wastes a debug session. Fail fast here instead.
pub fn fael_bin() -> PathBuf {
    let crew = PathBuf::from(env!("CARGO_BIN_EXE_fael-crew"));
    let fael = crew.with_file_name(format!("fael{}", std::env::consts::EXE_SUFFIX));
    assert!(
        fael.is_file(),
        "{} is missing — run `cargo test --workspace` (or `cargo build -p fael`) first",
        fael.display()
    );
    // Freshness by content, not mtime: the test links the current
    // `fael-core`, so every verb it knows must appear in the `fael`
    // binary's help. A stale `fael` prints the old list — rebuild both.
    let out = Command::new(&fael)
        .args(["plan", "--help"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    // `show` is an alias for `export`: named in PLAN_VERBS but not in the usage line.
    let missing: Vec<&&str> = fael_core::work::PLAN_VERBS
        .iter()
        .filter(|v| **v != "show" && !out.contains(**v))
        .collect();
    assert!(
        missing.is_empty(),
        "stale {} — `fael plan --help` lacks {missing:?} (partial build: only fael-crew rebuilt); run `cargo test --workspace` (or `cargo build --workspace`) first",
        fael.display()
    );
    fael
}
