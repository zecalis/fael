//! Who holds a start now: the live run, the pair members, the worktree
//! slot and the after-block — the guards `Store::start` checks before it
//! claims anything. Split out of `start.rs` (file-size ratchet).

use super::board_math::under;
use super::ready::{Ready, ready_in};
use super::shared::unshared;
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

/// Live-run chunks of the same app, outside the pair `ms`, whose scope (less `shared`) shares
/// a path prefix with a member's (the board's `overlap` rule): one `uid (plan/label): path, …`
/// line each. Judges nothing.
pub(super) fn clashes(
    tx: &Transaction,
    ms: &[String],
    shared: &[String],
) -> Result<Vec<String>, String> {
    let paths = |s: Option<String>| -> Vec<String> {
        let v: Vec<String> = s.iter().flat_map(|s| s.lines()).map(String::from).collect();
        unshared(&v, shared)
    };
    let mut mine = Vec::new();
    let mut apps = Vec::new();
    for m in ms {
        let (s, app): (Option<String>, String) = tx
            .query_row(
                "SELECT c.scope, p.app FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.uid = ?1",
                [m],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(err)?;
        mine.extend(paths(s));
        apps.push(app);
    }
    let Some(app) = apps.first() else {
        return Ok(Vec::new());
    };
    let mut q = tx
        .prepare(
            "SELECT c.uid, COALESCE(c.label, c.title), p.name, c.scope FROM chunk c
             JOIN plan p ON p.id = c.plan
             WHERE c.scope IS NOT NULL AND p.app = ?1 AND EXISTS (
               SELECT 1 FROM run r WHERE r.chunk = c.id AND r.ended IS NULL)",
        )
        .map_err(err)?;
    let rows = q
        .query_map([app], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get(3)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut out = Vec::new();
    for (uid, label, plan, scope) in rows {
        if ms.contains(&uid) {
            continue;
        }
        let theirs = paths(scope);
        let hit: Vec<_> = mine
            .iter()
            .filter(|x| theirs.iter().any(|y| under(x, y) || under(y, x)))
            .cloned()
            .collect();
        if !hit.is_empty() {
            out.push(format!("{uid} ({plan}/{label}): {}", hit.join(", ")));
        }
    }
    Ok(out)
}
