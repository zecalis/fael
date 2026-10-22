//! Cross-repo owner lookup and the ledger-repo registry (issue 01M4XKM4).
//!
//! One helper for both binaries — `fael` (`find`, `close`, MCP `show`) and
//! `fael-crew` (`chunk show`) read the same candidates through here, so the
//! two cannot drift. Read only on the reject path.
//!
//! A ledger exists on this machine only if fael wrote it (add, sync, import),
//! so every ledger write registers its main checkout root in the state file
//! `repos` — no guessing, no filesystem scan. The board's `projects` file is
//! crew's (every entry shows on the board); this one is core's and stays off
//! it. `usage` names row-only repos visited before the registry existed, and
//! `registered()` keeps board repos resolving.

use crate::{Repo, repo_at};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A row this repo does not hold but exactly one other repo on this machine
/// does. ULIDs never collide across machines, so one hit is the row — reads
/// may follow it, writes never do.
pub struct Elsewhere {
    pub at: PathBuf,
    pub kind: String,
    pub id: String,
}

/// The registry: one main-checkout root per line. Core-owned (`projects` is
/// the board's and stays untouched).
fn repos_path() -> PathBuf {
    crate::stats::state_dir().join("repos")
}

fn read_list(path: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Append `main` to the registry. Fail-open (never fails a write), deduped,
/// scratch and missing paths left out.
pub fn register_repo(main: &Path) {
    if main.as_os_str().is_empty() || !main.exists() {
        return;
    }
    if crate::work::is_scratch(main) {
        return;
    }
    let path = repos_path();
    if read_list(&path).iter().any(|p| p == main) {
        return;
    }
    if path
        .parent()
        .is_some_and(|d| std::fs::create_dir_all(d).is_err())
    {
        return;
    }
    use std::io::Write;
    let line = format!("{}\n", main.display());
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// The main checkout a ledger write belongs to: `<main>/.git/fael` is the
/// journal, else `<root>/.fael`. `None` for scratch layouts (`FAEL_DIR`
/// without a `.fael` leaf) and anything unrecognised — never registered.
fn store_main(fael: &Path, journal: Option<&Path>) -> Option<PathBuf> {
    if let Some(j) = journal {
        let main = j
            .parent()
            .filter(|g| g.file_name().is_some_and(|n| n == ".git"))
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        if main.is_some() {
            return main;
        }
    }
    if fael.file_name().is_some_and(|n| n == ".fael") {
        return fael.parent().map(Path::to_path_buf);
    }
    None
}

/// Register the repo a ledger write landed in. Called by every core ledger
/// write (add/close/bump/mv/restore via `write_both`, import via
/// `write_import`); the sync-ingest adapter calls `register_repo` itself.
pub(crate) fn register_store(fael: &Path, journal: Option<&Path>) {
    if let Some(m) = store_main(fael, journal) {
        register_repo(&m);
    }
}

/// Every repo fael knows: the registry, then the board's registry, then every
/// distinct `"repo"` usage names. Scratch repos and missing paths left out.
pub fn repos() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for p in read_list(&repos_path())
        .into_iter()
        .chain(crate::work::registered())
        .chain(usage_repos())
    {
        if p.as_os_str().is_empty() || !p.exists() || crate::work::is_scratch(&p) {
            continue;
        }
        if seen.insert(p.clone()) {
            out.push(p);
        }
    }
    out
}

/// Distinct `"repo"` values from the usage log: the live file plus the newest
/// archive only (bounded — never the whole history). Same fast slice as the
/// old `fael` reader: split on `"repo":"`, unescape once per distinct raw.
fn usage_repos() -> Vec<PathBuf> {
    let text = usage_text();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = vec![];
    for l in text.lines().filter_map(|l| l.split_once("\"repo\":\"")) {
        let raw = l.1.split('"').next().unwrap_or("");
        if raw.is_empty() || !seen.insert(raw) {
            continue;
        }
        if let Ok(s) = serde_json::from_str::<String>(&format!("\"{raw}\"")) {
            let p = PathBuf::from(s);
            if !p.as_os_str().is_empty() {
                out.push(p);
            }
        }
    }
    out
}

fn usage_text() -> String {
    let state = crate::stats::state_dir();
    let live = state.join("usage.jsonl");
    // newest archive only, like the usage reader's no-`since` path
    let newest: Option<PathBuf> = std::fs::read_dir(state.join("usage"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .max();
    newest
        .into_iter()
        .chain([live])
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

/// The unique owner of `tok` outside `r`, if there is exactly one. Two repos
/// holding the prefix stays a reject: a prefix is an address, not a name.
pub fn row_elsewhere(r: &Repo, tok: &str) -> Option<Elsewhere> {
    let mut seen = HashSet::from([crate::work::main_root_for(&r.root, r.journal.as_deref())]);
    let mut hit: Option<Elsewhere> = None;
    for path in repos() {
        let Ok(o) = repo_at(&path) else {
            continue;
        };
        let at = crate::work::main_root_for(&o.root, o.journal.as_deref());
        if !seen.insert(at.clone()) {
            continue;
        }
        if let Ok(row) = crate::resolve_row(&crate::read_repo(&o), tok) {
            if hit.is_some() {
                return None;
            }
            hit = Some(Elsewhere {
                at,
                kind: row.kind.clone(),
                id: row.id.clone(),
            });
        }
    }
    hit
}

/// The repo root holding `tok` as a chunk, if exactly one does. Labels never
/// cross repos — they are per plan — so only a uid or prefix resolves.
pub fn chunk_elsewhere(r: &Repo, tok: &str) -> Option<PathBuf> {
    if !crate::looks_like_id(tok) {
        return None;
    }
    let mut seen = HashSet::from([crate::work::main_root_for(&r.root, r.journal.as_deref())]);
    let mut hit: Option<PathBuf> = None;
    for path in repos() {
        let Ok(o) = repo_at(&path) else {
            continue;
        };
        let at = crate::work::main_root_for(&o.root, o.journal.as_deref());
        if !seen.insert(at.clone()) {
            continue;
        }
        if is_chunk(&o, tok) {
            if hit.is_some() {
                return None;
            }
            hit = Some(at);
        }
    }
    hit
}

fn is_chunk(o: &Repo, tok: &str) -> bool {
    let db = crate::work::db_for(o.journal.as_deref(), &o.fael);
    crate::work::is_chunk(&db, tok)
}

#[cfg(test)]
mod tests {
    use super::{repos, store_main};
    use std::path::Path;

    #[test]
    fn repos_never_panics_on_a_missing_state_dir() {
        let _ = repos();
    }

    #[test]
    fn store_main_resolves_git_journal_tree_fael_and_skips_scratch() {
        assert_eq!(
            store_main(Path::new("/w/.fael"), Some(Path::new("/w/.git/fael"))),
            Some(Path::new("/w").to_path_buf())
        );
        assert_eq!(
            store_main(Path::new("/w/.fael"), None),
            Some(Path::new("/w").to_path_buf())
        );
        assert_eq!(store_main(Path::new("/tmp/scratch-log"), None), None);
    }
}
