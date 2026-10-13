//! `project.branches[]` (decision 01M4V0FY: a lead stacks sets on a local branch no worktree
//! holds): every local branch but the base's own, by what git alone says — offline, no gh;
//! whether its PR is open is the app's. `chunks`: the unfinished chunks whose run branch it
//! holds, so a stack names the lanes landed on it.

use super::git_output;
use crate::git;
use fael_core::plan::BoardChunk;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
pub(super) struct Branch {
    name: String,
    /// `local` never pushed · `pushed` a live upstream · `merged` its content is on the base,
    /// or its upstream is gone (a merged PR the remote deleted)
    state: &'static str,
    chunks: Vec<String>,
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
            let state = state(root, base, tree.as_deref(), name, up, track);
            let chunks = live
                .iter()
                .filter(|(_, b)| state != "merged" && holds(root, name, b))
                .map(|(u, _)| u.to_string())
                .collect();
            Some(Branch {
                name: name.into(),
                state,
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
) -> &'static str {
    let ahead = git(root, &["rev-list", "--count", &format!("{base}..{b}")]);
    if track == "[gone]" || ahead.as_deref() == Some("0") {
        return "merged";
    }
    if !up.is_empty() {
        return "pushed";
    }
    // merging it changes nothing: every change is on the base already (a squash merge)
    let merged = git_output(root, &["merge-tree", "--write-tree", base, b])
        .and_then(|(_, out)| out.lines().next().map(String::from));
    if merged.is_some() && merged.as_deref() == tree {
        "merged"
    } else {
        "local"
    }
}

/// `branch` holds run branch `run`: it is that branch, or `run`'s tip is in its history.
fn holds(root: &Path, branch: &str, run: &str) -> bool {
    run == branch
        || git_output(root, &["merge-base", "--is-ancestor", run, branch])
            .is_some_and(|(code, _)| code == Some(0))
}
