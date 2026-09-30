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

use super::git::{fetch_refs, line_ref, line_sha, ls_prefix, read_tree};
use crate::{Repo, core};

/// Fetch, validate and ingest every ref under `refs/fael/<repo-id>/` except
/// `own`. Returns the count of rows appended. The whole pass costs one
/// `ls-remote`, one `fetch` and one journal read however many writers there
/// are: the listing already carries every tip sha, and each ref's rows are
/// added to the in-memory log so the next ref dedupes against them.
pub(super) fn others(r: &Repo, remote: &str, repo_id: &str, own: &str) -> Result<usize, String> {
    let Some(listing) = ls_prefix(&r.root, remote, &format!("refs/fael/{repo_id}/"))? else {
        return Ok(0);
    };
    let refs: Vec<(&str, &str)> = listing
        .lines()
        .map(|l| (line_ref(l), line_sha(l)))
        .filter(|(name, sha)| *name != own && !sha.is_empty())
        .collect();
    let names: Vec<&str> = refs.iter().map(|(name, _)| *name).collect();
    // one ref gone between the listing and the fetch fails the batch; then
    // each ref fetches for itself and only that one is skipped
    let batched = !names.is_empty() && fetch_refs(&r.root, remote, &names).is_ok();
    let mut log = crate::read(r);
    let mut n = 0;
    for (rname, sha) in refs {
        if !batched && let Err(e) = fetch_refs(&r.root, remote, &[rname]) {
            eprintln!("fael: skipped {rname} — {e}");
            continue;
        }
        match one(r, repo_id, rname, sha, &mut log) {
            Ok(k) => n += k,
            Err(e) => eprintln!("fael: skipped {rname} — {e}"),
        }
    }
    Ok(n)
}

fn one(
    r: &Repo,
    repo_id: &str,
    rname: &str,
    sha: &str,
    log: &mut core::Log,
) -> Result<usize, String> {
    let fetched = read_tree(&r.root, sha, rname)?;
    let Some(m) = fetched.meta else { return Ok(0) };
    core::sync::validate(&m, repo_id).map_err(|e| e.to_string())?;
    let n = ingest(r, log, &fetched.rows, &fetched.closes)?;
    log.rows.extend(fetched.rows);
    log.closes.extend(fetched.closes);
    Ok(n)
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
