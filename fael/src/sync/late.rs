//! Rows that have not reached their destination yet (PLAN-fael-local-first
//! chunk 2) — late is fine, lost is not, silent is not. `store = "tracked"`:
//! `.fael/log` files git sees as uncommitted. `local` with a remote: this
//! writer's rows newer than the last good sync, shown only once a sync failed
//! (a pending row is just late; the next sync carries it). The Stop receipt
//! and `doctor` both print [`line`].
//!
//! Sync state is two files in the journal dir, per clone, never in a row:
//! `synced` (the writer's newest id when the last good sync started) and
//! `sync-error` (the last failure's message, removed by the next success).

use crate::{Repo, core};
use std::path::{Path, PathBuf};

const MARK: &str = "synced";
const ERR: &str = "sync-error";

/// This writer's newest row or close id — the autosync mark and the
/// watermark a good sync records.
pub(crate) fn newest(r: &Repo) -> String {
    let (by, log) = (crate::writer(r), crate::read(r));
    let mine = log.rows.iter().chain(&log.closes).filter(|x| x.by == by);
    mine.map(|x| x.id.as_str()).max().unwrap_or("").to_string()
}

fn state(r: &Repo, name: &str) -> Option<PathBuf> {
    r.journal.as_deref().map(|j| j.join(name))
}

/// After a sync ran: a success moves the watermark to `mark` (taken before
/// the push read the journal, so a row filed meanwhile still counts as late)
/// and clears the error; a failure keeps the watermark and records why.
/// Fails open, like every state write.
pub(crate) fn record(r: &Repo, mark: &str, res: &Result<(), String>) {
    let (Some(m), Some(e)) = (state(r, MARK), state(r, ERR)) else {
        return;
    };
    match res {
        Ok(()) => {
            let _ = std::fs::write(m, mark);
            let _ = std::fs::remove_file(e);
        }
        Err(msg) => {
            let _ = std::fs::write(e, msg);
        }
    }
}

/// The one late line for this repo, `None` when nothing is late.
pub(crate) fn line(r: &Repo) -> Option<String> {
    match r.cfg.store {
        core::Store::Tracked => uncommitted(&r.root),
        core::Store::Local => unsynced(r),
    }
}

fn uncommitted(root: &Path) -> Option<String> {
    let out = crate::git(
        root,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ".fael/log",
        ],
    )?;
    Some(format!(
        "fael: {} .fael/log file(s) uncommitted (store = tracked) — commit them, or `fael migrate local`",
        out.lines().count()
    ))
}

fn unsynced(r: &Repo) -> Option<String> {
    let err = std::fs::read_to_string(state(r, ERR)?).ok()?;
    let mark = state(r, MARK)
        .and_then(|m| std::fs::read_to_string(m).ok())
        .unwrap_or_default();
    let (by, log) = (crate::writer(r), crate::read(r));
    let n = log
        .rows
        .iter()
        .chain(&log.closes)
        .filter(|x| x.by == by && x.id.as_str() > mark.trim())
        .count();
    let why = err.lines().next().unwrap_or("").trim();
    (n > 0).then(|| {
        format!("fael: {n} row(s) only in this clone — last sync failed ({why}); run `fael sync`")
    })
}
