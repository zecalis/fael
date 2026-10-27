//! The issue ask across sessions: the edit ask is said once per session, so an
//! open issue nobody fixed was asked again in every new session that touched its
//! file (vela, 2026-10-24: 81 ask lines for 45 issues, 4 of 80 acted, all bumps,
//! 0 close). The edit hook is PostToolUse and the ask fires on an edit of the
//! issue's own file, so the bytes always moved — file state cannot tell a new
//! question from a repeat. The row can: when an ask is said, the newest bump
//! event of that issue is kept here, per clone (keyed by the clone-shared
//! journal, so every worktree of one clone shares it, like the log); a later
//! session asks again only
//! when the issue has a newer one (`fael bump`, a claim) or is a new version of
//! the row (a new id). Closure stays with the Fixed line.
//! Per-machine state, never in `.fael/`. Fails open everywhere: an unreadable
//! file is "never asked" (the ask is said as before), an unwritable one drops
//! the record.

use super::state::{session_key, state_dir};
use crate::core;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Backstop on top of the closed/superseded prune: the newest entries win.
const MAX_ENTRIES: usize = 1000;

/// issue id -> marker of the row when it was last asked
type Record = HashMap<String, String>;

fn path(root: &Path) -> PathBuf {
    let key = session_key(&root.to_string_lossy());
    state_dir().join("asked").join(format!("{key}.tsv"))
}

/// `<id>\t<marker>` per line; a bad line is skipped.
fn load(root: &Path) -> Record {
    let Ok(s) = std::fs::read_to_string(path(root)) else {
        return Record::new();
    };
    s.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, m)| (id.to_string(), m.to_string()))
        .collect()
}

/// The row's state: the id of its newest bump event (ids are time-ordered),
/// `none` when it was never bumped. No file read, no git.
fn marker(log: &core::Log, id: &str) -> String {
    log.rows
        .iter()
        .filter(|r| r.bumps.as_deref() == Some(id))
        .map(|r| r.id.as_str())
        .max()
        .unwrap_or("none")
        .to_string()
}

/// Ids whose last ask is still current: asked before, and the row has not
/// changed since. No record, or a newer marker, means the ask is due.
pub(crate) fn already_asked(rows: &[&core::Row], log: &core::Log, root: &Path) -> HashSet<String> {
    let rec = load(root);
    rows.iter()
        .filter(|r| rec.get(&r.id).is_some_and(|m| *m == marker(log, &r.id)))
        .map(|r| r.id.clone())
        .collect()
}

/// Remember the issues just asked. Bounded: entries of closed or superseded
/// issues are dropped (they can never be asked again), then the newest
/// `MAX_ENTRIES` stay — the file cannot grow past the open issues.
pub(crate) fn record(log: &core::Log, root: &Path, asked: &[&core::Row]) {
    if asked.is_empty() {
        return;
    }
    let mut rec = load(root);
    for r in asked {
        rec.insert(r.id.clone(), marker(log, &r.id));
    }
    let (closed, gone) = (core::closed(log), core::superseded(log));
    rec.retain(|id, _| !closed.contains(id.as_str()) && !gone.contains(id.as_str()));
    // ids are time-ordered, so the largest are the newest
    let mut ids: Vec<&String> = rec.keys().collect();
    ids.sort();
    let body: String = ids
        .iter()
        .rev()
        .take(MAX_ENTRIES)
        .map(|id| format!("{id}\t{}\n", rec[*id]))
        .collect();
    let p = path(root);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(tmp, p);
    }
}
