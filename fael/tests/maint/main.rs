//! Chunk 6 through the real binary: doctor exit codes + --fix, compact,
//! import (incl. the fapony legacy no-drop rule) — each in a throwaway repo.
//!
//! Thin entry only — the suites sit next to this file:
//! `doctor` (union/fix, gone, stale, phantom, quarantine), `notenglish`
//! ([lang] rows foreign-row batch), `branches` (orphan + merged
//! branch notes), `compact` (close folding), `import` (fapony legacy).

mod branches;
mod compact;
mod doctor;
mod import;
mod journal_only;
mod notenglish;
mod shipped;
mod superseded;

use std::path::{Path, PathBuf};
use std::process::Command;

/// Per-child `FAEL_STATE_DIR` at `<repo root>/state`, so a real session on this
/// machine never leaks in and tests run in parallel without a global env lock.
// doctor/compact/import never read the state dir today; the env is set anyway
// so a future session read cannot leak across children.
fn state_env(c: &mut Command, dir: &Path) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    c.env("FAEL_STATE_DIR", root.join("state"));
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(dir);
    state_env(&mut c, dir);
    let o = c.output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-maint-cli-{}", fael_core::ulid()));
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
