//! Epoch-upgrade backups: before a breaking-epoch step moves `user_version` off
//! `v`, a copy of the db at `v` lands beside it. Split out of `schema.rs` for
//! the 400-line cap.

use super::schema::VERSION;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Before an epoch step moves `user_version` off `v`: a copy of the db at `v` beside it,
/// `<db>.v<v>.bak`. Err = refuse the upgrade. One is kept: the copy this upgrade just
/// made — an older one for the same `v` goes (issue 01M56WGH). Fail-open on the delete.
pub(super) fn backup(db: &Path, v: i32) -> Result<(), String> {
    let to = copy(db, &format!("v{v}"))
        .map_err(|e| format!("crew.db v{v} → v{VERSION} refused: {e}; nothing changed"))?;
    if let (Some(dir), Some(name)) = (to.parent(), db.file_name()) {
        let prefix = format!("{}.v{v}.", name.to_string_lossy());
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p != to
                    && let Some(n) = p.file_name().and_then(|n| n.to_str())
                    && n.starts_with(&prefix)
                    && n.ends_with(".bak")
                {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }
    Ok(())
}

/// `<db>.<tag>.bak` (an older one is kept: the new copy gets a ulid). The caller may hold
/// the write lock, so the copy goes through a second connection — a WAL reader sees the
/// last commit, and VACUUM cannot run inside the caller's transaction.
pub(super) fn copy(db: &Path, tag: &str) -> Result<PathBuf, String> {
    let mut to = PathBuf::from(format!("{}.{tag}.bak", db.display()));
    if to.exists() {
        to = PathBuf::from(format!("{}.{tag}.{}.bak", db.display(), fael_core::ulid()));
    }
    Connection::open(db)
        .and_then(|c| {
            c.busy_timeout(Duration::from_secs(10))?;
            c.execute("VACUUM INTO ?1", [to.to_string_lossy()])
        })
        .map(|_| to.clone())
        .map_err(|e| format!("the backup to {} failed ({e})", to.display()))
}
