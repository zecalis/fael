//! Retired notes (decision 01M5528X, row lifecycle D1): a note tied to a chunk or a
//! branch is done with when `fael chunk done` ended the chunk it was filed in, or git
//! proves its branch merged. Derived on every pull, never written — the log stays
//! append-only. Unproven = still live; fael never reads the note's text to decide.

use crate::Repo;
use fael_core::{self as core, work::EndedRun};
use std::collections::HashSet;

/// `plan:<name>:handoff` / `plan:<name>:chunk-<n>` → the plan name.
fn plan_of(key: &str) -> Option<&str> {
    let rest = key.strip_prefix("plan:")?;
    let (plan, tail) = rest.rsplit_once(':')?;
    (tail == "handoff"
        || tail
            .strip_prefix("chunk-")
            .is_some_and(|n| n.parse::<u32>().is_ok()))
    .then_some(plan)
}

/// Did a done run end the chunk this note was filed in? Its branch is the run's, or its
/// session is the run's and its key names the run's plan; a note written after the run
/// ended belongs to later work.
fn chunk_done(row: &core::Row, ends: &[EndedRun]) -> bool {
    let plan = row.key.as_deref().and_then(plan_of);
    let born = core::ts_ms(&row.ts);
    ends.iter().any(|e| {
        let before = matches!((born, core::ts_ms(&e.ended)), (Some(b), Some(x)) if b <= x);
        before
            && (row.branch().is_some_and(|b| e.branch.as_deref() == Some(b))
                || (plan == Some(e.plan.as_str())
                    && row
                        .session()
                        .is_some_and(|s| e.session.as_deref() == Some(s))))
    })
}

/// Ids of the retired notes among the open rows. `git` adds the merged-branch proof
/// (`find::view`, a fixed number of spawns per pull however many branches the repo
/// holds) and writes what it proved to the state cache. The push path passes false — it owns no spawn — and
/// adds the cached set only while HEAD, its branch and the row count are the pull's.
pub(crate) fn ids(r: &Repo, log: &core::Log, git: bool) -> HashSet<String> {
    let key = crate::journal::head_sha(&r.root).map(|h| {
        let b = crate::journal::head_branch(&r.root).unwrap_or_default();
        format!("{h} {} {b}", log.rows.len())
    });
    let mut out = derive(r, log, git);
    if let Some(k) = key {
        if git {
            super::state::write_retired(&r.root, &k, &out);
        } else if let Some(c) = super::state::read_retired(&r.root, &k) {
            out.extend(c);
        }
    }
    out
}

fn derive(r: &Repo, log: &core::Log, git: bool) -> HashSet<String> {
    let hide: HashSet<&str> = core::closed(log)
        .union(&core::superseded(log))
        .copied()
        .collect();
    let head = crate::journal::head_branch(&r.root);
    let db = core::work::db_for(r.journal.as_deref(), &r.fael);
    let ends = core::work::ended_runs(&db);
    let notes = log
        .rows
        .iter()
        .filter(|w| w.kind == "note" && w.held().is_none() && !hide.contains(w.id.as_str()));
    let mut out = HashSet::new();
    // the notes whose branch git must say merged, asked together: one pull however
    // many branches there are, and none at all when no note is a candidate
    let mut ask: Vec<(&core::Row, &str)> = vec![];
    for w in notes {
        let tied = w.branch().filter(|b| !b.is_empty());
        let handoff = w.key.as_deref().and_then(plan_of).is_some();
        if (tied.is_some() || handoff) && chunk_done(w, &ends) {
            out.insert(w.id.clone());
        } else if let Some(b) = tied
            // a handoff waits on its chunk, not the merge; a revisit waits on its date
            && git
            && w.revisit.is_none()
            && !handoff
            // the branch being worked on and the trunk are not "merged work"
            && Some(b) != head.as_deref()
            && !crate::journal::is_trunk(&r.root, b)
        {
            ask.push((w, b));
        }
    }
    let items: Vec<(&str, Option<&str>)> = ask.iter().map(|(w, b)| (*b, w.sha())).collect();
    let merged = crate::find::merged_branches(&r.root, &items);
    out.extend(
        ask.iter()
            .zip(merged)
            .filter(|(_, m)| *m)
            .map(|((w, _), _)| w.id.clone()),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::plan_of;

    #[test]
    fn only_the_handoff_and_chunk_keys_name_a_plan() {
        assert_eq!(plan_of("plan:demo:handoff"), Some("demo"));
        assert_eq!(plan_of("plan:demo:chunk-3"), Some("demo"));
        assert_eq!(plan_of("plan:demo"), None);
        assert_eq!(plan_of("plan:demo:chunk-x"), None);
        assert_eq!(plan_of("add:shape-gate"), None);
    }
}
