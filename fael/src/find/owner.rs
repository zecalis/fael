//! Where a missing id lives (issue 01M4XKM4): a chunk in this repo's crew.db, or a row or
//! chunk in another repo on this machine — the board's registry and every repo usage names.
//! Read only on the reject path: an agent that was told "no row" hunted repo by repo.

use crate::{Repo, core};
use std::collections::HashSet;
use std::path::PathBuf;

/// The line naming `tok`'s owner, if it is anything but missing everywhere.
pub(crate) fn owner(r: &Repo, tok: &str) -> Option<String> {
    if is_chunk(r, tok) {
        return Some(format!(
            "{tok} is a chunk here — `fael chunk show {tok}` reads it"
        ));
    }
    let mut seen = HashSet::from([core::work::main_root_for(&r.root, r.journal.as_deref())]);
    for path in repos() {
        let Ok(o) = crate::repo_at(&path) else {
            continue;
        };
        let at = core::work::main_root_for(&o.root, o.journal.as_deref());
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
    let db = core::work::db_for(r.journal.as_deref(), &r.fael);
    core::work::is_chunk(&db, tok)
}

/// A chunk uid of this repo passed to find: its brief, as MCP `chunk show` prints it. The
/// "is a chunk here" hint alone still cost agents a round (opencode, 2026-10-17).
pub(crate) fn chunk_here(r: &Repo, tok: &str) -> Option<String> {
    let db = core::work::db_for(r.journal.as_deref(), &r.fael);
    core::work::chunk_brief(&db, tok).map(|b| brief(&b))
}

/// The chunk-show text: title, uid, plan, brief.
pub(crate) fn brief(b: &core::work::Brief) -> String {
    format!(
        "# {} ({})\nplan {} — {}\n\n{}\n",
        b.title, b.uid, b.plan, b.plan_title, b.brief
    )
}

/// The board's repos, then every distinct `"repo"` in usage — scratch repos and paths that
/// are gone left out.
pub(crate) fn repos() -> Vec<PathBuf> {
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
    core::work::registered()
        .into_iter()
        .chain(named.collect::<Vec<_>>())
        .filter(|p| !p.as_os_str().is_empty() && p.exists() && !core::work::is_scratch(p))
        .collect()
}
