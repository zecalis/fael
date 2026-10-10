//! The chunk a session takes next in one plan, read from `plans.db` — fapony
//! `pickChunks` (parallel.ts) over the db instead of the TL;DR lines, so an import and
//! `fapony plan` agree on the same files. b3's queue (pin → due → unblocks) builds on it.

use super::store::Store;
use rusqlite::params;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Chunk {
        label: Option<String>,
        text: String,
    },
    /// open chunks, none startable (claimed elsewhere, waiting, or waiting on a chunk)
    NoneReady,
    Blocked,
    Parked,
    /// every chunk done or dropped
    Closed,
    /// the plan sits in done/
    Shipped,
}

/// `branch` = this session's branch (a claim naming it is this session's); `live` = the
/// branches some worktree has checked out (`None` = unknown: every claim holds).
pub fn next(
    s: &Store,
    plan: i64,
    branch: Option<&str>,
    live: Option<&HashSet<String>>,
) -> Result<Next, String> {
    let e = |e: rusqlite::Error| format!("plans.db: {e}");
    let state: String = s
        .conn
        .query_row("SELECT state FROM plan WHERE id = ?1", [plan], |r| r.get(0))
        .map_err(e)?;
    match state.as_str() {
        "parked" => return Ok(Next::Parked),
        "done" => return Ok(Next::Shipped),
        "blocked" => return Ok(Next::Blocked),
        _ => {}
    }
    let mut q = s
        .conn
        .prepare(
            "SELECT c.id, c.label, c.title, c.state,
               (SELECT branch FROM run WHERE chunk = c.id ORDER BY id DESC LIMIT 1),
               (SELECT COUNT(*) FROM dep d LEFT JOIN chunk a ON a.id = d.after
                 WHERE d.chunk = c.id AND (a.id IS NULL OR a.state NOT IN ('done','dropped')))
             FROM chunk c
             WHERE c.plan = ?1 AND c.state IN ('open','running','waiting')
             ORDER BY c.pos",
        )
        .map_err(e)?;
    type Open = (Option<String>, String, String, Option<String>, i64);
    let open: Vec<Open> = q
        .query_map(params![plan], |r| {
            Ok((r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)?;
    if open.is_empty() {
        return Ok(Next::Closed);
    }
    let mut mine = None;
    let mut ready = None;
    for (i, (_, _, st, claim, unmet)) in open.iter().enumerate() {
        if st == "running" {
            let b = claim.as_deref().unwrap_or("");
            // a claim no worktree holds (merged, abandoned) is picked like an open chunk
            let stale = !b.is_empty() && Some(b) != branch && live.is_some_and(|l| !l.contains(b));
            if !stale {
                if mine.is_none() && (b.is_empty() || Some(b) == branch) {
                    mine = Some(i);
                }
                continue;
            }
        } else if st == "waiting" {
            continue;
        }
        if *unmet == 0 && ready.is_none() {
            ready = Some(i);
        }
    }
    Ok(match mine.or(ready) {
        Some(i) => Next::Chunk {
            label: open[i].0.clone(),
            text: open[i].1.clone(),
        },
        None => Next::NoneReady,
    })
}
