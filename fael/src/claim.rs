//! `fael claim <id>` — say which branch works on an open issue, so a second
//! agent sees it in `find --kind issue` (`held @<branch>`) before picking it
//! up. A bump that stamps `held` with this branch: informational, never a
//! lock — a second claim moves it, with a warning naming the old holder.

use crate::{Repo, core, read};
use std::path::PathBuf;

pub(crate) fn claim(r: &Repo, id: &str) -> Result<(core::Row, PathBuf, Vec<String>), String> {
    let stamp = crate::stamp(r);
    let Some(branch) = stamp.branch.clone() else {
        return Err("rejected: claim names the branch holding the issue — this checkout is on no branch (detached HEAD or no git); git switch -c <branch> first".into());
    };
    let log = read(r);
    let old = core::resolve(&log, id)?;
    if old.kind != "issue" {
        return Err(format!(
            "rejected: {} is a {} — claim takes an open issue",
            old.id, old.kind
        ));
    }
    let mut warns = vec![];
    match old.held() {
        Some(h) if h == branch => return Err(format!("rejected: {} is already held @{h}", old.id)),
        Some(h) => warns.push(format!(
            "warning: {} was held @{h} — now @{branch}; check that branch is not still on it",
            old.id
        )),
        None => {}
    }
    let (row, path, mut more) = core::bump_row(
        &r.fael,
        r.journal.as_deref(),
        &log,
        &r.cfg,
        &stamp,
        id,
        core::BumpOpts {
            to: None,
            urgent: core::UrgentChange::Keep,
            revisit: None,
            held: Some(branch),
        },
    )?;
    warns.append(&mut more);
    Ok((row, path, warns))
}
