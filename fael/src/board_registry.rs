//! The board's registry (SPEC §10): every repo whose fael.db a fael command wrote, so
//! `fael board` runs from anywhere — scratch repos under the temp dir stay off it.

use crate::Repo;
use std::path::{Path, PathBuf};

/// `~/.local/state/fael/projects`: one main-checkout root per line — every repo the board
/// covers (SPEC §10). The app runs `fael board` from home, outside any repo.
fn registry() -> PathBuf {
    fael_core::stats::state_dir().join("projects")
}

pub(super) fn registered() -> Vec<PathBuf> {
    let text = std::fs::read_to_string(registry()).unwrap_or_default();
    let all: Vec<PathBuf> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    let keep: Vec<PathBuf> = all.iter().filter(|p| !scratch(p)).cloned().collect();
    // prune what older faels registered (issue 01M4MH3H7); a failed write only skips again
    if keep.len() < all.len() {
        let text: String = keep.iter().map(|p| format!("{}\n", p.display())).collect();
        let _ = std::fs::write(registry(), text);
    }
    keep
}

/// A repo under the temp dir (an agent's or a test's scratch repo) belongs only on a
/// registry that is itself under the temp dir — a test's `FAEL_STATE_DIR`.
fn scratch(root: &Path) -> bool {
    in_tmp(root) && !in_tmp(&fael_core::stats::state_dir())
}

/// Under the temp dir — canonical (`/private/var/…` on macOS) through the nearest dir
/// that exists yet.
pub(crate) fn in_tmp(p: &Path) -> bool {
    let p = p
        .ancestors()
        .find_map(|a| Some(a.canonicalize().ok()?.join(p.strip_prefix(a).ok()?)))
        .unwrap_or_else(|| p.to_path_buf());
    [std::env::temp_dir(), PathBuf::from("/tmp")]
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .any(|d| p.starts_with(d))
}

/// Put this repo on the board: called by every command that writes its fael.db.
pub(crate) fn register(r: &Repo) {
    // the main checkout, not this worktree: `<main>/.git/fael` is the journal
    let root = r
        .journal
        .as_deref()
        .and_then(Path::parent)
        .filter(|g| g.file_name().is_some_and(|n| n == ".git"))
        .and_then(Path::parent)
        .unwrap_or(&r.root);
    if scratch(root) || registered().iter().any(|p| p == root) {
        return;
    }
    let f = registry();
    let line = format!("{}\n", root.display());
    let ok = f
        .parent()
        .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
        && std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&f)
            .and_then(|mut h| std::io::Write::write_all(&mut h, line.as_bytes()))
            .is_ok();
    if !ok {
        eprintln!("fael: could not add {} to {}", root.display(), f.display());
    }
}

/// `fael board --drop <root>`: take a finished repo off the board (issue 01M4NC9GR). The
/// next `fael chunk …` or `fael plan …` write in it puts it back.
pub(super) fn unregister(root: &str) -> Result<std::process::ExitCode, String> {
    let want = Path::new(root);
    let want = want.canonicalize().unwrap_or_else(|_| want.to_path_buf());
    let all = registered();
    let keep: Vec<&PathBuf> = all
        .iter()
        .filter(|p| p.canonicalize().as_ref().unwrap_or(p) != &want)
        .collect();
    if keep.len() == all.len() {
        return Err(format!(
            "rejected: {} is not on the board — `fael board` lists its roots",
            want.display()
        ));
    }
    let text: String = keep.iter().map(|p| format!("{}\n", p.display())).collect();
    std::fs::write(registry(), text).map_err(|e| format!("{}: {e}", registry().display()))?;
    println!("dropped {}", want.display());
    Ok(std::process::ExitCode::SUCCESS)
}
