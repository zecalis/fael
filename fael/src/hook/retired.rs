//! Retired notes (decision 01M5528X, row lifecycle D1): a note tied to a chunk or a
//! branch is done with when `fael chunk done` ended the chunk it was filed in, or git
//! proves its branch merged. Derived on every pull, never written — the log stays
//! append-only. Unproven = still live; fael never reads the note's text to decide.

use crate::Repo;
use fael_core::{self as core, plan::EndedRun};
use std::collections::{HashMap, HashSet};

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
/// (`find::merged`, batched to three spawns per pull however many branches the
/// repo holds, with the per-branch squash proof as fallback): the push path
/// passes false — it owns no spawn.
pub(crate) fn ids(r: &Repo, log: &core::Log, git: bool) -> HashSet<String> {
    let hide: HashSet<&str> = core::closed(log)
        .union(&core::superseded(log))
        .copied()
        .collect();
    let head = crate::journal::head_branch(&r.root);
    let mut proved: HashMap<&str, bool> = HashMap::new();
    // one `for-each-ref --merged` per pull, not one proof per branch; lazily
    // built by the first merge candidate, so a pull with none spawns nothing
    let mut merged: Option<HashSet<String>> = None;
    let db = crate::plan::db(r);
    let ends = if db.exists() {
        core::plan::Store::open(&db)
            .and_then(|s| s.ended_runs())
            .unwrap_or_default()
    } else {
        vec![]
    };
    log.rows
        .iter()
        .filter(|w| w.kind == "note" && w.held().is_none() && !hide.contains(w.id.as_str()))
        .filter(|w| {
            let tied = w.branch().filter(|b| !b.is_empty());
            let handoff = w.key.as_deref().and_then(plan_of).is_some();
            (tied.is_some() || handoff) && chunk_done(w, &ends)
                // a handoff waits on its chunk, not the merge; a revisit waits on its date
                || git
                    && w.revisit.is_none()
                    && !handoff
                    // the branch being worked on and the trunk are not "merged work"
                    && tied.is_some_and(|b| {
                        Some(b) != head.as_deref() && !crate::journal::is_trunk(&r.root, b)
                    })
                    && tied.is_some_and(|b| {
                        *proved.entry(b).or_insert_with(|| {
                            let set = merged
                                .get_or_insert_with(|| crate::find::merged_names(&r.root));
                            // the batch proves regular merges; a squash merge
                            // is no ancestor, so it still takes the per-branch
                            // patch-id proof (empty batch = that path as before)
                            set.contains(b) || crate::find::merged_branch(&r.root, b)
                        })
                    })
        })
        .map(|w| w.id.clone())
        .collect()
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
