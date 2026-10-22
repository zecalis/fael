//! Where a chunk uid lives when this repo's crew.db does not hold it (plan
//! fael-friction-2 show-elsewhere): read-only, never running the command
//! elsewhere. Labels are per plan, so only a uid or an 8+-char prefix
//! resolves across repos.
//!
//! Thin over `fael-core::work::elsewhere` — the scan itself lives there so
//! `fael` and `fael-crew` cannot drift.

use crate::{Repo, core};
use std::path::{Path, PathBuf};

/// The main-checkout root holding `tok`, if it is a chunk uid (or prefix) in
/// another repo fael knows. `None` for labels and for uids nowhere holds.
pub(crate) fn elsewhere(r: &Repo, tok: &str) -> Option<PathBuf> {
    core::work::chunk_elsewhere(r, tok)
}

/// A full uid names one chunk (uids are fixed-length ULIDs, so the lookup is
/// exact); a shorter prefix can open chunks in several repos, so the line says
/// what it established and no more. Keeps the `lives in` needle the
/// `chunk_missing` hook code matches on.
pub(crate) fn message(tok: &str, root: &Path) -> String {
    let what = if tok.len() == 26 {
        format!("chunk {tok}")
    } else {
        format!("a chunk starting with {tok}")
    };
    format!(
        "rejected: {what} lives in {} — cd {} && fael chunk show {tok}",
        root.display(),
        root.display(),
    )
}
