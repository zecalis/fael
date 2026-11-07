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
/// what it established and no more. The prose line keeps the `lives in` needle
/// the `chunk_missing` hook code and existing tests match on; the
/// `fael-elsewhere cwd=<root>` line is the parse contract MCP reads
/// (issue 01M5J6M1) — prose may drift, the machine line may not.
pub(crate) fn message(tok: &str, root: &Path) -> String {
    let what = if tok.len() == 26 {
        format!("chunk {tok}")
    } else {
        format!("a chunk starting with {tok}")
    };
    format!(
        "rejected: {what} lives in {} — cd {} && fael chunk show {tok}\nfael-elsewhere cwd={}",
        root.display(),
        root.display(),
        root.display(),
    )
}

#[cfg(test)]
mod tests {
    use super::message;
    use std::path::Path;

    #[test]
    fn elsewhere_message_keeps_prose_and_machine_line() {
        let m = message("01M5J6M1000000000000000000", Path::new("/w/other"));
        assert!(m.contains("lives in /w/other"), "{m}");
        assert!(m.contains("fael-elsewhere cwd=/w/other"), "{m}");
        let m = message("01M5J6M1", Path::new("/w/other"));
        assert!(m.contains("a chunk starting with 01M5J6M1 lives in"), "{m}");
        assert!(
            m.lines().any(|l| l
                .strip_prefix("fael-elsewhere cwd=")
                .is_some_and(|c| !c.trim().is_empty())),
            "{m}"
        );
    }
}
