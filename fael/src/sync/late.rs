//! Rows that have not reached their destination yet (PLAN-fael-local-first
//! chunk 2) — late is fine, lost is not, silent is not. `store = "tracked"`:
//! `.fael/log` files git sees as uncommitted. `local` with a remote: this
//! writer's rows newer than the last good sync, shown only once a sync failed
//! (a pending row is just late; the next sync carries it). `local` with no
//! remote in a repo whose `.fael/log` is in git: the team used to share by
//! commit, and new rows no longer do. The Stop receipt and `doctor` both
//! print [`line`].
//!
//! Sync state is two files in the journal dir, per clone, never in a row:
//! `synced` (the writer's newest id when the last good sync to `fael.remote`
//! started) and `sync-error` (that remote's last failure, credentials
//! stripped, removed by the next success).

use crate::{Repo, core};
use std::path::{Path, PathBuf};

const MARK: &str = "synced";
const ERR: &str = "sync-error";

/// This writer's newest row or close id — the autosync mark.
pub(crate) fn newest(r: &Repo) -> String {
    newest_in(&crate::read(r), &crate::writer(r))
}

/// [`newest`] over a log the caller already read — the watermark a good sync
/// records, taken from the log its push read.
pub(super) fn newest_in(log: &core::Log, by: &str) -> String {
    let mine = log.rows.iter().chain(&log.closes).filter(|x| x.by == by);
    mine.map(|x| x.id.as_str()).max().unwrap_or("").to_string()
}

fn state(r: &Repo, name: &str) -> Option<PathBuf> {
    r.journal.as_deref().map(|j| j.join(name))
}

/// After a sync to `fael.remote` ran: a success moves the watermark to its
/// mark (taken when the push read the journal, so a row filed meanwhile
/// still counts as late) and clears the error; a failure keeps the
/// watermark and records why. Fails open, like every state write.
pub(super) fn record(r: &Repo, res: &Result<String, String>) {
    let (Some(m), Some(e)) = (state(r, MARK), state(r, ERR)) else {
        return;
    };
    match res {
        Ok(mark) => {
            let _ = std::fs::write(m, mark);
            let _ = std::fs::remove_file(e);
        }
        Err(msg) => {
            let _ = std::fs::write(e, core::sync::strip_userinfo(msg));
        }
    }
}

/// Rows came in that this clone pushes but are older than the watermark (an
/// import): forget it, so a failed sync says rows may be late instead of
/// counting none.
// ponytail: rows filed under an earlier user.email of this clone after the
// last good sync are not counted either; forget the mark on a writer change
// too if that ever bites
pub(crate) fn forget(r: &Repo) {
    if let Some(m) = state(r, MARK) {
        let _ = std::fs::remove_file(m);
    }
}

/// The one late line for this repo, `None` when nothing is late. `log` is
/// the union read the caller already holds.
pub(crate) fn line(r: &Repo, log: &core::Log) -> Option<String> {
    match r.cfg.store {
        core::Store::Tracked => uncommitted(&r.root),
        core::Store::Local => unsynced(r, log).or_else(|| unshared(r)),
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

fn unsynced(r: &Repo, log: &core::Log) -> Option<String> {
    let err = std::fs::read_to_string(state(r, ERR)?).ok()?;
    let why = err.lines().next().unwrap_or("").trim();
    // no watermark (first sync after an upgrade, or an import): a count would
    // be every row this writer ever filed — say what is known instead
    let Some(mark) = state(r, MARK).and_then(|m| std::fs::read_to_string(m).ok()) else {
        return Some(format!(
            "fael: last sync failed ({why}) — rows since the last good sync may be only in this clone; run `fael sync`"
        ));
    };
    let by = crate::writer(r);
    let n = log
        .rows
        .iter()
        .chain(&log.closes)
        .filter(|x| x.by == by && x.id.as_str() > mark.trim())
        .count();
    (n > 0).then(|| {
        format!("fael: {n} row(s) only in this clone — last sync failed ({why}); run `fael sync`")
    })
}

/// Unset `store`, a `.fael/log` in git and no `fael.remote`: the repo shared
/// memory by commit, and since unset `store` became `local` new rows stay in
/// this clone. An explicit `store = "local"` chose that. Spawns git only
/// when a tree log is on disk.
// ponytail: up to two git spawns per Stop in such a repo until it is set up
// (the receipt dedups after); cache per session if Stop latency matters
fn unshared(r: &Repo) -> Option<String> {
    if r.cfg.store_set
        || !r.fael.join("log").is_dir()
        || crate::git(&r.root, &["config", "fael.remote"]).is_some()
        || crate::git(&r.root, &["ls-files", "--", ".fael/log"]).is_none()
    {
        return None;
    }
    Some(
        "fael: new rows stay in this clone — .fael/log in git is frozen history under store = local; \
         share them with `fael sync --remote <url>`, or set store = \"tracked\" to commit them again"
            .into(),
    )
}
