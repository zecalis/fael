//! Git transport for the writer journal (PLAN-fael-journal-transport chunk 3):
//! `fael sync [--remote url]` carries the local journal to `refs/fael/<repo-id>/<writer>`
//! on a remote and ingests every writer's ref back into the local journal.
//!
//! Pure journal work (`Meta`, ref names, month-split files, id-union, meta
//! validation) lives in `fael_core::sync` — this module only spawns git
//! plumbing (see `git`) and appends missing rows through `fael_core::append`,
//! the same raw write `import`/`compact` use. A `.close.jsonl` row without
//! `ref` (which an import can carry) rides along: it could never pass
//! `close()`'s `validate_close`, so ingest never validates, it only dedupes
//! by id — and skips any row that trips the shared secret check.
//!
//! The working tree and checked-out branches are never touched: fetches land
//! in `FETCH_HEAD`, pushes name the commit sha directly.

mod git;

use crate::{Repo, core};
use git::{fetch_tree, line_ref, ls_prefix, ls_remote, push, tip_tree, write_tree};
use std::path::Path;

/// `fael sync [--remote url]`: push this writer's journal, ingest every
/// writer's. Prints one summary line; an empty journal against a remote with
/// no ref prints `nothing to sync` and creates nothing.
pub(crate) fn sync(r: &Repo, a: &crate::Args) -> Result<(), String> {
    let remote = remote(r, a.one("remote"))?;
    warn_origin(r, &remote);
    let repo_id = repo_id(r)?;
    let by = crate::writer(r);
    let own = core::sync::ref_name(&repo_id, &by)?;
    let origin = crate::git(&r.root, &["config", "remote.origin.url"]).unwrap_or_default();
    let name = name(&r.root);
    let meta = core::sync::Meta::new(&repo_id, &origin, &name);

    // own ref, before and after: ls → fetch+union → commit → ff-only push.
    let mut tip = ls_remote(&r.root, &remote, &own)?;
    let log = crate::read(r);
    let local_rows: Vec<core::Row> = local_writer(&log.rows, &by);
    let local_closes: Vec<core::Row> = local_writer(&log.closes, &by);
    let (mut pushed, mut ingested) = (0usize, 0usize);
    if !local_rows.is_empty() || !local_closes.is_empty() || tip.is_some() {
        let attempt = |tip: &Option<String>| -> Result<(Vec<core::Row>, Vec<core::Row>), String> {
            let fetched = match tip {
                Some(sha) => fetch_tree(&r.root, &remote, &own, sha)?,
                None => git::Fetched::default(),
            };
            if let Some(m) = fetched.meta {
                core::sync::validate(&m, &repo_id).map_err(|e| format!("{own}: {e}"))?;
            }
            Ok((
                core::sync::union(&fetched.rows, &local_rows),
                core::sync::union(&fetched.closes, &local_closes),
            ))
        };
        push_one(
            r,
            &remote,
            &own,
            &meta,
            &log,
            &mut tip,
            &mut pushed,
            &mut ingested,
            &attempt,
        )?;
    }
    // every other writer's ref under this repo-id: fetch, validate, ingest.
    if let Some(refs) = ls_prefix(&r.root, &remote, &format!("refs/fael/{repo_id}/"))? {
        for rname in refs.lines().map(line_ref).filter(|n| *n != own) {
            let sha = ls_remote(&r.root, &remote, rname)?.unwrap_or_default();
            if sha.is_empty() {
                continue;
            }
            let fetched = fetch_tree(&r.root, &remote, rname, &sha)?;
            let Some(m) = fetched.meta else { continue };
            if core::sync::validate(&m, &repo_id).is_err() {
                continue;
            }
            let log = crate::read(r);
            ingested += ingest(r, &log, &fetched.rows, &fetched.closes)?;
        }
    }
    if pushed == 0 && ingested == 0 && tip.is_none() {
        println!("nothing to sync");
    } else {
        println!("synced: pushed {pushed}, ingested {ingested}");
    }
    Ok(())
}

/// Fetch+union for one tip: the merged journal both sides converge on.
type Attempt<'a> = &'a dyn Fn(&Option<String>) -> Result<(Vec<core::Row>, Vec<core::Row>), String>;

/// Push this writer's union for one tip: fetch+union, ingest what is missing
/// locally, and push when the union differs from the remote tree. A remote
/// that moved under us re-fetches, re-unions and retries once; a second
/// failure is the caller's next `fael sync`.
#[allow(clippy::too_many_arguments)]
fn push_one(
    r: &Repo,
    remote: &str,
    own: &str,
    meta: &core::sync::Meta,
    log: &core::Log,
    tip: &mut Option<String>,
    pushed: &mut usize,
    ingested: &mut usize,
    attempt: Attempt<'_>,
) -> Result<(), String> {
    let (rows, closes) = attempt(tip)?;
    *ingested += ingest(r, log, &rows, &closes)?;
    let files = core::sync::tree_files(meta, &rows, &closes);
    let tree = write_tree(&r.root, &files)?;
    // the union is what the remote already has — no commit, no push.
    if tip_tree(&r.root, tip)? == Some(tree.clone()) {
        *pushed = 0;
        return Ok(());
    }
    *pushed = rows.len() + closes.len();
    let sha = git::commit_tree(&r.root, &tree, tip.as_deref())?;
    if push(&r.root, remote, own, &sha)?.is_some() {
        return Ok(());
    }
    *tip = ls_remote(&r.root, remote, own)?;
    let (rows, closes) = attempt(tip)?;
    let files = core::sync::tree_files(meta, &rows, &closes);
    let tree = write_tree(&r.root, &files)?;
    if tip_tree(&r.root, tip)? == Some(tree.clone()) {
        *pushed = 0;
        return Ok(());
    }
    let sha = git::commit_tree(&r.root, &tree, tip.as_deref())?;
    if push(&r.root, remote, own, &sha)?.is_none() {
        return Err("fael: remote moved twice during sync — run fael sync again".into());
    }
    Ok(())
}

/// This writer's filed rows — what its ref carries. Rows others filed live
/// locally after ingest but belong to their writers' refs, never this one.
fn local_writer(rows: &[core::Row], by: &str) -> Vec<core::Row> {
    rows.iter().filter(|r| r.by == by).cloned().collect()
}

/// Append fetched rows missing locally, one stream at a time so the two
/// dedupe separately exactly as the reader does. Returns the count appended.
fn ingest(
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
            put(r, &row, is_close)?;
            n += 1;
        }
    }
    Ok(n)
}

/// The journal-first write for an ingested row: journal, then the tree per
/// `store` (`local` skips the tree) — the same order as `add_row`, without
/// its validation (fetched bytes already parsed; see the module docs).
fn put(r: &Repo, row: &core::Row, is_close: bool) -> Result<(), String> {
    if let Some(j) = r.journal.as_deref() {
        core::append(j, row, is_close)?;
    }
    if !matches!(r.cfg.store, core::Store::Local) || r.journal.is_none() {
        core::append(&r.fael, row, is_close)?;
    }
    Ok(())
}

/// `--remote <url>` wins, else `git config fael.remote` (per machine, in
/// `.git/config`, never committed). Neither is set → the one-line error.
fn remote(r: &Repo, flag: Option<String>) -> Result<String, String> {
    if let Some(u) = flag.filter(|u| !u.is_empty()) {
        return Ok(u);
    }
    crate::git(&r.root, &["config", "fael.remote"]).ok_or_else(|| {
        "fael: no fael.remote — set it with: git config fael.remote <url>".to_string()
    })
}

/// `store = local` + destination is `origin`: the ref is fetchable by anyone
/// with read access even though no UI shows it. One line, then push as usual.
fn warn_origin(r: &Repo, remote: &str) {
    if !matches!(r.cfg.store, core::Store::Local) {
        return;
    }
    let origin = crate::git(&r.root, &["config", "remote.origin.url"]).unwrap_or_default();
    if !origin.is_empty() && remote == origin {
        eprintln!(
            "fael: fael ref is publicly fetchable from origin — point fael.remote at a private remote if this repo is public"
        );
    }
}

/// The workspace identity, identical for every clone and every branch: the
/// minimum root sha, cached in `git config fael.repoid` at the first sync so
/// the value never moves. `refs/fael/*` is excluded so sync's own parentless
/// commits can never shift it. A shallow clone errors instead of a wrong id.
fn repo_id(r: &Repo) -> Result<String, String> {
    if let Some(id) = crate::git(&r.root, &["config", "fael.repoid"]) {
        return Ok(id);
    }
    if !Path::new(&r.root.join(".git")).exists() {
        return Err("fael: not a git repo — sync needs git".to_string());
    }
    if crate::git(&r.root, &["rev-parse", "--is-shallow-repository"]).as_deref() == Some("true") {
        return Err(
            "fael: shallow clone — run `git fetch --unshallow`, then `fael sync` again".to_string(),
        );
    }
    let roots = git::run(
        &r.root,
        &[
            "rev-list",
            "--max-parents=0",
            "--all",
            "--not",
            "--glob=refs/fael/*",
        ],
    )?;
    let id = roots.lines().map(str::trim).filter(|l| !l.is_empty()).min();
    let Some(id) = id else {
        return Err("fael: no commits yet — commit something, then `fael sync`".to_string());
    };
    let _ = git::run(&r.root, &["config", "fael.repoid", id]);
    Ok(id.to_string())
}

/// Short repo name for `meta.json`'s display label — the checkout dir name.
fn name(root: &Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
