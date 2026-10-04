//! `fael claim <id>` and `fael next` — say which branch works on an open
//! issue, so a second agent sees it in `find --kind issue` (`held @<branch>`)
//! and picks another. A bump that stamps `held` with this branch.
//!
//! The check and the write are one step: claimers in one clone (every
//! worktree shares it) take `.claim.lock` in the journal, so of two agents
//! racing for an issue exactly one wins and the other is told who holds it.
//! It gates the claim, never the work: `--force` takes an issue over, a hold
//! whose branch no longer exists is taken over on its own, and nothing here
//! blocks an edit. Across machines a hold is only as fresh as the last
//! `fael sync` — there the lock cannot help.

use crate::{Repo, core, read};
use std::path::{Path, PathBuf};

type Claimed = (core::Row, PathBuf, Vec<String>);

/// One claimer at a time per clone; dropped with the returned file. No git, no
/// journal, no lock — and no branch to claim with either.
fn exclusive(r: &Repo) -> Result<Option<std::fs::File>, String> {
    let Some(dir) = r.journal.as_deref() else {
        return Ok(None);
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(".claim.lock"))
        .map_err(|e| format!("lock: {e}"))?;
    f.lock().map_err(|e| format!("lock: {e}"))?;
    Ok(Some(f))
}

/// Does the branch holding an issue still exist in this clone? A hold made on
/// another machine is stale here until its branch is fetched.
fn alive(r: &Repo, branch: &str) -> bool {
    r.journal
        .as_deref()
        .and_then(Path::parent)
        .is_some_and(|common| crate::journal::branch_alive(common, branch))
}

fn me(r: &Repo) -> Result<(core::Stamp, String), String> {
    let stamp = crate::stamp(r);
    let Some(branch) = stamp.branch.clone() else {
        return Err("rejected: claim names the branch holding the issue — this checkout is on no branch (detached HEAD or no git); git switch -c <branch> first".into());
    };
    Ok((stamp, branch))
}

pub(crate) fn claim(r: &Repo, id: &str, force: bool) -> Result<Claimed, String> {
    let _lock = exclusive(r)?;
    let (stamp, branch) = me(r)?;
    let log = read(r);
    take(r, &log, &stamp, &branch, id, force)
}

/// The best issue to work on next, claimed in the same step: open, not waiting,
/// routed to nobody or to this reader, not held by a branch that still exists —
/// ranked like every list (yours, then urgent, then newest).
pub(crate) fn next(r: &Repo, reader: &str) -> Result<Claimed, String> {
    let _lock = exclusive(r)?;
    let (stamp, branch) = me(r)?;
    let log = read(r);
    let day = core::today();
    let ready: Vec<&core::Row> = core::find(&log, &core::Filter::default())
        .into_iter()
        .filter(|x| x.kind == "issue")
        .filter(|x| x.to_who().is_none_or(|t| core::to_matches(t, reader)))
        .filter(|x| !core::row_waiting(x, &day))
        .filter(|x| x.held().is_none_or(|h| h != branch && !alive(r, h)))
        .collect();
    let first = core::ranked(ready, Some(reader), |_| 0, core::fresh_ts)
        .into_iter()
        .next()
        .ok_or("rejected: no ready issue — every open one is held, waiting or routed to someone else (fael find --kind issue)")?;
    take(r, &log, &stamp, &branch, &first.id.clone(), false)
}

fn take(
    r: &Repo,
    log: &core::Log,
    stamp: &core::Stamp,
    branch: &str,
    id: &str,
    force: bool,
) -> Result<Claimed, String> {
    let old = core::resolve_row(log, id)?;
    if old.kind != "issue" {
        return Err(format!(
            "rejected: {} is a {} — claim takes an open issue",
            old.id, old.kind
        ));
    }
    let mut warns = vec![];
    match old.held() {
        Some(h) if h == branch => return Err(format!("rejected: {} is already held @{h}", old.id)),
        Some(h) if alive(r, h) && !force => {
            return Err(format!(
                "rejected: {} is held @{h} — fael next picks a free issue; --force takes it over",
                old.id
            ));
        }
        Some(h) => warns.push(format!(
            "warning: {} was held @{h}{} — now @{branch}",
            old.id,
            if alive(r, h) { "" } else { " (branch gone)" }
        )),
        None => {}
    }
    let (row, path, mut more) = core::bump_row(
        &r.fael,
        r.journal.as_deref(),
        log,
        &r.cfg,
        stamp,
        id,
        core::BumpOpts {
            to: None,
            urgent: core::UrgentChange::Keep,
            revisit: None,
            held: Some(branch.to_string()),
            // a claim is not a check: carry the row's old hashes, never restamp
            fh: old.file_hashes().cloned(),
        },
    )?;
    warns.append(&mut more);
    Ok((row, path, warns))
}
