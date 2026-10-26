//! `project.branches[]` (decision 01M4V0FY: a lead stacks sets on a local branch no worktree
//! holds): every local branch but the base's own, by what git alone says — offline, no gh;
//! whether its PR is open is the app's. `chunks`: the unfinished chunks whose run branch it
//! holds, so a stack names the lanes landed on it.

use super::git_output;
use crate::git;
use fael_crew::BoardChunk;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub(super) struct Branch {
    pub(super) name: String,
    /// `local` never pushed · `pushed` a live upstream of its own name · `merged` its content is on the base,
    /// or its upstream is gone (a merged PR the remote deleted)
    pub(super) state: &'static str,
    /// content on the base: ahead 0 or merge-tree == base tree. A gone upstream alone is not
    /// (a PR closed unmerged loses its branch too); `accept --merged` reads this, not `state`
    #[serde(skip)]
    pub(super) on_base: bool,
    pub(super) chunks: Vec<String>,
}

/// Empty for a repo with no `origin/HEAD`: nothing to push to, nothing said.
pub(super) fn branches(root: &Path, base: Option<&str>, chunks: &[BoardChunk]) -> Vec<Branch> {
    let Some(base) = base else {
        return Vec::new();
    };
    let fmt = "--format=%(refname:short)%09%(upstream)%09%(upstream:track)";
    // untrimmed: a last branch with no upstream ends in tabs `git()` would cut
    let refs = git_output(root, &["for-each-ref", fmt, "refs/heads"])
        .map(|(_, out)| out)
        .unwrap_or_default();
    let tree = git(root, &["rev-parse", &format!("{base}^{{tree}}")]);
    let live: Vec<(&str, &str)> = chunks
        .iter()
        .filter(|c| matches!(c.state.as_str(), "running" | "waiting" | "review"))
        .filter_map(|c| Some((c.uid.as_str(), c.run.as_ref()?.branch.as_deref()?)))
        // a run on the base's own branch is in every branch's history: no branch holds it
        .filter(|(_, b)| Some(*b) != base.strip_prefix("origin/"))
        .collect();
    refs.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (name, up, track) = (f.next()?, f.next()?, f.next().unwrap_or(""));
            if Some(name) == base.strip_prefix("origin/") {
                return None;
            }
            let (state, on_base) = state(root, base, tree.as_deref(), name, up, track);
            let chunks = live
                .iter()
                .filter(|(_, b)| state != "merged" && holds(root, name, b))
                .map(|(u, _)| u.to_string())
                .collect();
            Some(Branch {
                name: name.into(),
                state,
                on_base,
                chunks,
            })
        })
        .collect()
}

fn state(
    root: &Path,
    base: &str,
    tree: Option<&str>,
    b: &str,
    up: &str,
    track: &str,
) -> (&'static str, bool) {
    let ahead = git(root, &["rev-list", "--count", &format!("{base}..{b}")]);
    if ahead.as_deref() == Some("0") {
        return ("merged", true);
    }
    // merging it changes nothing: every change is on the base already (a squash merge)
    let squashed = || {
        let merged = git_output(root, &["merge-tree", "--write-tree", base, b])
            .and_then(|(_, out)| out.lines().next().map(String::from));
        merged.is_some() && merged.as_deref() == tree
    };
    if track == "[gone]" {
        return ("merged", squashed());
    }
    // its own name on a remote: `git branch x origin/main` tracks main, x is not pushed
    if up.ends_with(&format!("/{b}")) {
        return ("pushed", false);
    }
    if squashed() {
        ("merged", true)
    } else {
        ("local", false)
    }
}

/// A review chunk whose run branch is a merged branch (content on the base) says `merged`.
/// A run that handed in a file (`--out`) names a branch it may never have committed to:
/// ahead 0 would read as merged, so it is left alone.
pub(super) fn mark_merged(chunks: &mut [BoardChunk], branches: &[Branch]) {
    for c in chunks.iter_mut().filter(|c| c.state == "review") {
        let Some(run) = c.run.as_ref().filter(|r| r.out.is_none()) else {
            continue;
        };
        c.merged = run
            .branch
            .as_deref()
            .is_some_and(|b| branches.iter().any(|x| x.on_base && x.name == b));
    }
}

/// The uids `fael chunk accept --merged` takes: this repo's review chunks on a merged branch.
pub(crate) fn merged_reviews(r: &crate::Repo) -> Result<Vec<String>, String> {
    let (today, stale) = super::clock();
    let mut b = fael_crew::Store::open(&crate::plan::db(r))?.board(&today, &stale)?;
    let base = git(&r.root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]);
    let branches = branches(&r.root, base.as_deref(), &b.chunks);
    mark_merged(&mut b.chunks, &branches);
    Ok(b.chunks
        .into_iter()
        .filter(|c| c.merged)
        .map(|c| c.uid)
        .collect())
}

/// `branch` holds run branch `run`: it is that branch, or `run`'s tip is in its history.
fn holds(root: &Path, branch: &str, run: &str) -> bool {
    run == branch
        || git_output(root, &["merge-base", "--is-ancestor", run, branch])
            .is_some_and(|(code, _)| code == Some(0))
}
