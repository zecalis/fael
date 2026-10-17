//! Who holds a start now: the live run, the pair members, the worktree
//! slot and the after-block — the guards `Store::start` checks before it
//! claims anything. Split out of `start.rs` (file-size ratchet).

use super::ready::{Ready, ready_in};
use super::store::err;
use rusqlite::{OptionalExtension, Transaction};

/// The chunk and every chunk a `pair` edge joins to it, in plan order.
pub(super) fn members(tx: &Transaction, id: i64) -> Result<Vec<String>, String> {
    let mut q = tx
        .prepare(
            "SELECT c.uid FROM chunk c WHERE c.id = ?1 OR c.id IN (
               SELECT dst FROM edge WHERE src = ?1 AND kind = 'pair'
               UNION SELECT src FROM edge WHERE dst = ?1 AND kind = 'pair')
             ORDER BY c.plan, c.seq",
        )
        .map_err(err)?;
    q.query_map([id], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}

/// Who holds a running chunk now: its live run, if any.
pub(super) fn holder(tx: &Transaction, id: i64) -> Result<Option<String>, String> {
    tx.query_row(
        "SELECT start || ' (' || COALESCE(client, 'agent') || ') in ' || COALESCE(worktree, '?')
                || COALESCE(' on ' || branch, '') || ' since ' || started
         FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
        [id],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
}

/// The live run's id and session, if any — for a same-session retry of
/// `chunk start` to count as success instead of demanding `--force`.
pub(super) fn holder_detail(
    tx: &Transaction,
    id: i64,
) -> Result<Option<(String, Option<String>)>, String> {
    tx.query_row(
        "SELECT start, session FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(err)
}

/// The live start R in a worktree, if any.
pub(super) fn busy(tx: &Transaction, worktree: &str) -> Result<Option<String>, String> {
    tx.query_row(
        "SELECT start FROM run WHERE worktree = ?1 AND ended IS NULL LIMIT 1",
        [worktree],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
}

/// `ready_in`, but an after on a chunk of its own pair is met by plan order: one run does both.
pub(super) fn ready_paired(
    tx: &Transaction,
    id: i64,
    today: &str,
    pair: &[String],
) -> Result<Ready, String> {
    Ok(match ready_in(tx, id, today)? {
        Ready::Blocked(u) => {
            let u: Vec<_> = u
                .into_iter()
                .filter(|u| !u.uid.as_ref().is_some_and(|x| pair.contains(x)))
                .collect();
            if u.is_empty() {
                Ready::Yes
            } else {
                Ready::Blocked(u)
            }
        }
        r => r,
    })
}
