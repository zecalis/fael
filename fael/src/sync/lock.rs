//! One `fael sync` at a time per clone. The journal lives in the git common
//! dir, so every worktree of a clone shares it, and `missing()` is computed
//! from a log read before appending: two syncs at once (auto syncs from
//! several worktrees, or manual plus auto) would both append the same
//! ingested rows and leave duplicate lines per id. The lock is an advisory
//! `flock` on `<journal>/sync.lock`, held until the guard drops or the
//! process dies — a crashed sync never leaves a stale lock behind.

use crate::Repo;
use std::fs::{File, OpenOptions, TryLockError};

/// Take the clone's sync lock, or say another sync holds it. `None` guard
/// without a journal (no readable `.git`; sync stops at `repo_id` then anyway).
pub(super) fn acquire(r: &Repo) -> Result<Option<File>, String> {
    let Some(dir) = r.journal.as_deref() else {
        return Ok(None);
    };
    let path = dir.join("sync.lock");
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(dir).map_err(io)?;
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(io)?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(TryLockError::WouldBlock) => Err(
            "fael: another sync is running in this clone — run fael sync again when it ends"
                .to_string(),
        ),
        Err(TryLockError::Error(e)) => Err(io(e)),
    }
}
