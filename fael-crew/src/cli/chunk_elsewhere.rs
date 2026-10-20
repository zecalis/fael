//! Where a chunk uid lives when this repo's crew.db does not hold it (plan
//! fael-friction-2 show-elsewhere): the board's registry plus every repo usage
//! names, read-only, never running the command elsewhere. Labels are per plan,
//! so only a uid or an 8+-char prefix resolves across repos.

use crate::{Repo, board, core};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The main-checkout root holding `tok`, if it is a chunk uid (or prefix) in
/// another repo fael knows. `None` for labels and for uids nowhere holds.
pub(crate) fn elsewhere(r: &Repo, tok: &str) -> Option<PathBuf> {
    if !core::looks_like_id(tok) {
        return None;
    }
    let mut seen = HashSet::from([board::main_root(r).to_path_buf()]);
    for path in repos() {
        let Ok(o) = crate::repo_at(&path) else {
            continue;
        };
        let at = board::main_root(&o).to_path_buf();
        if !seen.insert(at.clone()) {
            continue;
        }
        let db = crate::plan::db(&o);
        if !db.exists() {
            continue;
        }
        if fael_crew::Store::open(&db).is_ok_and(|s| s.is_chunk(tok)) {
            return Some(at);
        }
    }
    None
}

/// The board's repos, then every repo the usage log names — `fael` reads that log (the hook's),
/// so it is asked (`fael held repos`); asked only here, when a uid is not in this repo's db.
/// ponytail: if `fael` cannot run, the board's registry alone.
fn repos() -> Vec<PathBuf> {
    match crate::fael_bin::run(&["held", "repos"], "") {
        Ok(out) => out.lines().map(PathBuf::from).collect(),
        Err(_) => core::work::registered()
            .into_iter()
            .filter(|p| p.exists() && !core::work::is_scratch(p))
            .collect(),
    }
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
