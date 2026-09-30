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
//! The working tree, checked-out branches and `FETCH_HEAD` are never touched:
//! fetches only store objects, pushes name the commit sha directly.

mod git;
mod ingest;
mod lock;
mod origin;
mod purged;
mod push;

pub(crate) use purged::record as record_purge;

use crate::{Repo, core};
use std::path::Path;

/// `fael sync [--remote url]`: ingest every other writer's ref, then push this
/// writer's journal. Prints one summary line; an empty journal against a
/// remote with no ref prints `nothing to sync` and creates nothing.
///
/// Ingest comes first: the push needs to know which rows other refs already
/// carry, which writer ids own a ref, and which ids were purged.
pub(crate) fn sync(r: &Repo, a: &crate::Args) -> Result<(), String> {
    let _lock = lock::acquire(r)?; // before the first journal read; drops at return
    let remote = remote(r, a.one("remote"))?;
    origin::warn(r, &remote);
    let repo_id = repo_id(r)?;
    let by = crate::writer(r);
    let refname = core::sync::ref_name(&repo_id, &by)?;
    let origin = crate::git(&r.root, &["config", "remote.origin.url"]).unwrap_or_default();
    let meta = core::sync::Meta::new(&repo_id, &origin, &name(&r.root));

    let others = ingest::others(r, &remote, &repo_id, &refname)?;
    let own = push::Own {
        r,
        remote: &remote,
        refname: &refname,
        repo_id: &repo_id,
        by: &by,
        meta: &meta,
    };
    let p = own.push(&others)?;
    let ingested = others.ingested + p.ingested;
    if p.pushed == 0 && ingested == 0 && others.own_tip.is_none() {
        println!("nothing to sync");
    } else {
        println!("synced: pushed {}, ingested {ingested}", p.pushed);
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

/// The workspace identity, identical for every clone and every branch: the
/// minimum root sha, cached in `git config fael.repoid` at the first sync so
/// the value never moves. Only branches and origin's branches count — what
/// every clone shares; `--all` would let a `stash -u`, `git notes`, another
/// remote or sync's own parentless commits shift it. A shallow clone errors
/// instead of a wrong id.
// ponytail: a local-only orphan branch, or a single-branch clone of a repo whose
// orphan branch holds the min root, still derives its own id — pin it with
// `git config fael.repoid <id>` if that ever bites.
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
            "--branches",
            "--remotes=origin",
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
