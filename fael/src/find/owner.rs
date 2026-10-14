//! Where a missing id lives (issue 01M4XKM4): a chunk in this repo's fael.db, or a row or
//! chunk in another repo on this machine — the board's registry and every repo usage names.
//! Read only on the reject path: an agent that was told "no row" hunted repo by repo.

use crate::{Repo, board, core};
use std::collections::HashSet;
use std::path::PathBuf;

/// The line naming `tok`'s owner, if it is anything but missing everywhere.
pub(crate) fn owner(r: &Repo, tok: &str) -> Option<String> {
    if is_chunk(r, tok) {
        return Some(format!(
            "{tok} is a chunk here — `fael chunk show {tok}` reads it"
        ));
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
        let at = at.display();
        if let Ok(row) = core::resolve_row(&crate::read(&o), tok) {
            return Some(format!(
                "{tok} is a ledger {} in {at} — `cd {at} && fael find {tok}`",
                row.kind
            ));
        }
        if is_chunk(&o, tok) {
            return Some(format!(
                "{tok} is a chunk in {at} — `cd {at} && fael chunk show {tok}`"
            ));
        }
    }
    None
}

fn is_chunk(r: &Repo, tok: &str) -> bool {
    Some(crate::plan::db(r))
        .filter(|p| p.exists())
        .and_then(|p| core::plan::Store::open(&p).ok())
        .is_some_and(|s| s.is_chunk(tok))
}

/// The board's repos, then every distinct `"repo"` in usage — scratch repos and paths that
/// are gone left out.
fn repos() -> Vec<PathBuf> {
    let usage = crate::hook::usage_files::read(None);
    let mut seen = HashSet::new();
    let named = usage.lines().filter_map(|l| {
        let raw = l.split_once("\"repo\":\"")?.1.split('"').next()?;
        // ponytail: dedupe the raw slice first, unescape once per repo (27 MB, ~8k repos)
        seen.insert(raw)
            .then(|| serde_json::from_str::<String>(&format!("\"{raw}\"")).ok())
            .flatten()
            .map(PathBuf::from)
    });
    board::registered()
        .into_iter()
        .chain(named.collect::<Vec<_>>())
        .filter(|p| !p.as_os_str().is_empty() && p.exists() && !board::scratch(p))
        .collect()
}
