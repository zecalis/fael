//! The push half of `fael sync`: this writer's own ref, built as
//! ls → fetch+union → tree → ff-only push, retried once when the remote moved.
//!
//! Which rows go on this ref (`carried`): the ones this writer filed, the ones
//! an import put here, and rows of a writer id that owns no ref (an earlier
//! `user.email`/`user.name` of this clone) that no other ref already carries.
//! `meta.json` is written by the first push only — a later push, from any
//! clone or worktree, keeps what the ref says (docs/sync-format.md).

use super::git::{Fetched, commit_tree, fetch_tree, ls_remote, push, tip_tree, write_tree};
use super::ingest::{Others, ingest};
use super::purged;
use crate::{Repo, core};
use std::collections::HashSet;

/// Everything one own-ref push needs from the caller.
pub(super) struct Own<'a> {
    pub r: &'a Repo,
    pub remote: &'a str,
    pub refname: &'a str,
    pub repo_id: &'a str,
    pub by: &'a str,
    /// The label for a first push; an existing ref keeps its own.
    pub meta: &'a core::sync::Meta,
}

#[derive(Default)]
pub(super) struct Pushed {
    /// Rows this push added to the ref — not the size of the whole union.
    pub pushed: usize,
    /// Rows the remote ref held that the journal was missing.
    pub ingested: usize,
    /// This writer's newest id in the journal this push read — the late
    /// watermark once the sync succeeds.
    pub mark: String,
}

impl Own<'_> {
    /// Push the union of the ref and the journal. A remote that moved under us
    /// re-fetches, re-unions, ingests what that brought and retries once; a
    /// second failure is the caller's next `fael sync`.
    pub(super) fn push(&self, o: &Others) -> Result<Pushed, String> {
        let r = self.r;
        let mut log = crate::journal::merged(r); // raw: never ship a folded row
        let imported = imported_ids(r);
        let carried = |rows: &[core::Row]| carried(rows, self.by, &imported, o);
        let (rows0, closes0) = (carried(&log.rows), carried(&log.closes));
        let mut tip = o.own_tip.clone();
        let mut out = Pushed {
            mark: super::late::newest_in(&log, self.by),
            ..Pushed::default()
        };
        if rows0.is_empty() && closes0.is_empty() && tip.is_none() {
            return Ok(out);
        }
        let local_purged = purged::local(r);
        for round in 0..2 {
            let fetched = match &tip {
                Some(sha) => fetch_tree(&r.root, self.remote, self.refname, sha)?,
                None => Fetched::default(),
            };
            let meta = match &fetched.meta {
                Some(m) => {
                    core::sync::validate(m, self.repo_id)
                        .map_err(|e| format!("{}: {e}", self.refname))?;
                    m
                }
                None => self.meta,
            };
            // tombstones filter the union; this ref carries only its own
            let filter = o.purged.iter().chain(&fetched.purged).cloned().collect();
            let tomb = local_purged
                .iter()
                .chain(&fetched.purged)
                .cloned()
                .collect();
            let rows = no_secrets(core::sync::without_purged(
                core::sync::union(&fetched.rows, &rows0),
                &filter,
            ));
            let closes = no_secrets(core::sync::without_purged(
                core::sync::union(&fetched.closes, &closes0),
                &filter,
            ));
            out.ingested += ingest(r, &log, &rows, &closes)?;
            log.rows.extend(rows.iter().cloned());
            log.closes.extend(closes.iter().cloned());
            let mut files = core::sync::tree_files(meta, &rows, &closes);
            files.extend(core::sync::purged_file(&tomb));
            let tree = write_tree(&r.root, &files)?;
            out.pushed = core::sync::missing(&rows, &fetched.rows).len()
                + core::sync::missing(&closes, &fetched.closes).len();
            // the union is what the remote already has — no commit, no push.
            if tip_tree(&r.root, &tip)? == Some(tree.clone()) {
                out.pushed = 0;
                return Ok(out);
            }
            let sha = commit_tree(&r.root, &tree, tip.as_deref())?;
            if push(&r.root, self.remote, self.refname, &sha)?.is_some() {
                return Ok(out);
            }
            if round == 0 {
                tip = ls_remote(&r.root, self.remote, self.refname)?;
            }
        }
        Err("fael: remote moved twice during sync — run fael sync again".into())
    }
}

/// The rows this ref carries (see the module docs). Rows others filed live
/// locally after ingest but belong to their writers' refs, never this one; an
/// imported row keeps its legacy `by` (`claude`) and is pushed by the clone
/// that imported it, once — the clones that receive it see it on that ref.
fn carried(rows: &[core::Row], by: &str, imported: &HashSet<String>, o: &Others) -> Vec<core::Row> {
    let mine = |r: &&core::Row| {
        r.by == by
            || (!o.seen.contains(&r.id) && (imported.contains(&r.id) || !o.writers.contains(&r.by)))
    };
    rows.iter().filter(mine).cloned().collect()
}

/// The push-side twin of ingest's check: a leaked row in this journal (from
/// before the add-time check, or an import) never leaves the machine, and one
/// an older fael already pushed drops out of the next tip. Label + id only.
fn no_secrets(rows: Vec<core::Row>) -> Vec<core::Row> {
    let clean = |row: &core::Row| {
        let Some(what) = core::secret(&row.to_line()) else {
            return true;
        };
        eprintln!(
            "fael: not pushing row {} — looks like a secret ({what}); rotate it, then `fael purge {}`",
            row.id, row.id
        );
        false
    };
    rows.into_iter().filter(clean).collect()
}

/// Ids of every row under `_import/`, in the tree and in the journal.
fn imported_ids(r: &Repo) -> HashSet<String> {
    let roots = std::iter::once(r.fael.as_path()).chain(r.journal.as_deref());
    let mut ids = HashSet::new();
    for log in roots.map(core::read_imported) {
        ids.extend(log.rows.into_iter().chain(log.closes).map(|x| x.id));
    }
    ids
}
