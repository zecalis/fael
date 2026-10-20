//! The ingest half of `fael sync`: every other writer's ref under this repo-id
//! is fetched, validated and appended to the local journal.
//!
//! One writer's bad ref never stops the team's sync: a ref whose `meta.json`
//! does not parse (what a `format_version` bump looks like to an older fael),
//! whose meta fails `validate`, or whose fetch fails is skipped with one
//! stderr line, and so is a row `append` refuses (bad `by`/`ts`) — the rest of
//! the ref still lands.
//!
//! Ingest writes the **journal only** (`<git-common-dir>/fael`), never the
//! working tree: a teammate's rows in `.fael/log/` would dirty `git status`,
//! ride the next `git add -A` into a PR and conflict with that writer's own
//! PR (docs/sync-format.md: never a working-tree file).

use super::git::{Fetched, fetch_refs, line_ref, line_sha, ls_prefix, read_tree};
use super::purged;
use crate::{Repo, core};
use std::collections::{BTreeSet, HashSet};

/// What one pass over the other writers' refs learned, for the push that follows.
#[derive(Default)]
pub(super) struct Others {
    /// Rows appended to the local journal.
    pub ingested: usize,
    /// This writer's own ref tip as the same listing saw it.
    pub own_tip: Option<String>,
    /// Row ids purged here or in any ref: no push carries them back.
    pub purged: BTreeSet<String>,
    /// Ids another ref already carries — a clone that received them must not
    /// push them again under its own ref.
    pub seen: HashSet<String>,
    /// Writer ids that own a ref.
    pub writers: HashSet<String>,
}

/// Fetch, validate and ingest every ref under `refs/fael/<repo-id>/` except
/// `own`. The whole pass costs one `ls-remote`, one `fetch` and one journal
/// read however many writers there are: the listing already carries every tip
/// sha, and each ref's rows are added to the in-memory log so the next ref
/// dedupes against them. Tombstones are gathered from every ref first, so a
/// purge in one ref also stops the same row arriving from another.
pub(super) fn others(r: &Repo, remote: &str, repo_id: &str, own: &str) -> Result<Others, String> {
    let mut out = Others {
        purged: purged::local(r),
        ..Others::default()
    };
    let Some(listing) = ls_prefix(&r.root, remote, &format!("refs/fael/{repo_id}/"))? else {
        return Ok(out);
    };
    let all: Vec<(&str, &str)> = listing
        .lines()
        .map(|l| (line_ref(l), line_sha(l)))
        .filter(|(_, sha)| !sha.is_empty())
        .collect();
    out.own_tip = all
        .iter()
        .find(|(name, _)| *name == own)
        .map(|(_, sha)| sha.to_string());
    let refs: Vec<(&str, &str)> = all.into_iter().filter(|(name, _)| *name != own).collect();
    out.writers = refs
        .iter()
        .filter_map(|(name, _)| name.rsplit('/').next().map(String::from))
        .collect();
    let names: Vec<&str> = refs.iter().map(|(name, _)| *name).collect();
    // one ref gone between the listing and the fetch fails the batch; then
    // each ref fetches for itself and only that one is skipped
    let batched = !names.is_empty() && fetch_refs(&r.root, remote, &names).is_ok();
    let mut trees = vec![];
    for (rname, sha) in refs {
        if !batched && let Err(e) = fetch_refs(&r.root, remote, &[rname]) {
            eprintln!("fael: skipped {rname} — {e}");
            continue;
        }
        match one(r, repo_id, sha, rname) {
            Ok(f) => trees.push(f),
            Err(e) => eprintln!("fael: skipped {rname} — {e}"),
        }
    }
    out.purged
        .extend(trees.iter().flat_map(|f| f.purged.clone()));
    let mut log = crate::journal::merged(r); // raw: never ship a folded row
    for f in trees {
        out.seen
            .extend(f.rows.iter().chain(&f.closes).map(|x| x.id.clone()));
        let rows = core::sync::without_purged(f.rows, &out.purged);
        let closes = core::sync::without_purged(f.closes, &out.purged);
        out.ingested += ingest(r, &log, &rows, &closes)?;
        log.rows.extend(rows);
        log.closes.extend(closes);
    }
    Ok(out)
}

/// One ref's tree, read and checked; a ref without `meta.json` reads empty.
fn one(r: &Repo, repo_id: &str, sha: &str, rname: &str) -> Result<Fetched, String> {
    let fetched = read_tree(&r.root, sha, rname)?;
    let Some(m) = &fetched.meta else {
        return Ok(Fetched::default());
    };
    core::sync::validate(m, repo_id).map_err(|e| e.to_string())?;
    Ok(fetched)
}

/// Append fetched rows missing locally, one stream at a time so the two
/// dedupe separately exactly as the reader does. Returns the count appended.
pub(super) fn ingest(
    r: &Repo,
    log: &core::Log,
    rows: &[core::Row],
    closes: &[core::Row],
) -> Result<usize, String> {
    let mut n = 0;
    for (fetched, is_close) in [(rows, false), (closes, true)] {
        let local = if is_close { &log.closes } else { &log.rows };
        for row in core::sync::missing(fetched, local) {
            // a leaked row is never carried in: it would be re-served under
            // every ref and come back after `fael purge`. Label + id only.
            if let Some(what) = core::secret(&row.to_line()) {
                eprintln!(
                    "fael: skipped row {} from {} — looks like a secret ({what}); rotate it, then `fael purge {}` at its source",
                    row.id, row.by, row.id
                );
                continue;
            }
            match put(r, &row, is_close) {
                Ok(()) => n += 1,
                // `append` refuses a `by` that is not a folder name or a `ts`
                // that is not RFC 3339; anything else (disk, permissions) is real
                Err(e) if e.starts_with("rejected:") => {
                    eprintln!("fael: skipped row {} from {} — {e}", row.id, row.by);
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(n)
}

/// The write for an ingested row: the journal, and only the journal — see the
/// module docs. Without a journal (no readable `.git`) the tree is all there
/// is, so the row goes there rather than nowhere. Fetched bytes already
/// parsed, so there is no `add`-style validation (see `super` docs).
fn put(r: &Repo, row: &core::Row, is_close: bool) -> Result<(), String> {
    let dir = r.journal.as_deref().unwrap_or(&r.fael);
    core::append(dir, row, is_close).map(drop)
}
