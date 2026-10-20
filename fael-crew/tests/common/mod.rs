//! Shared by the integration tests: these drive the `fael` binary, which hands the crew verbs to
//! `fael-crew`. Cargo builds only this package's own binary for a test, so `fael` is looked up
//! next to it — present after `cargo test --workspace`.

use std::path::PathBuf;

/// The `fael` binary built beside `fael-crew`.
pub fn fael_bin() -> PathBuf {
    let crew = PathBuf::from(env!("CARGO_BIN_EXE_fael-crew"));
    let fael = crew.with_file_name(format!("fael{}", std::env::consts::EXE_SUFFIX));
    assert!(
        fael.is_file(),
        "{} is missing — run `cargo test --workspace` (or `cargo build -p fael`) first",
        fael.display()
    );
    fael
}
