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

/// The worktree of the chunk's live run, if it has one — the seat that run sits in.
pub(super) fn holder_worktree(tx: &Transaction, id: i64) -> Result<Option<String>, String> {
    tx.query_row(
        "SELECT worktree FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
        [id],
        |r| r.get(0),
    )
    .optional()
    .map(Option::flatten)
    .map_err(err)
}

/// The live run's last touch when it is stalled (`last_seen` else `started` before
/// `stale`, the board's rule): `None` = no live run, or it was seen recently.
/// A stalled holder is no refusal — the caller takes it over with this as the
/// receipt (fael:01M6CZ9G41).
pub(super) fn stalled_since(
    tx: &Transaction,
    id: i64,
    stale: &str,
) -> Result<Option<String>, String> {
    let row: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT started, last_seen FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    Ok(row.and_then(|(started, seen)| {
        let at = seen.unwrap_or(started);
        (at.as_str() < stale).then_some(at)
    }))
}

/// End every live run in a worktree: one start per worktree, so a run still live there
/// belongs to an agent that left. The chunk stays `running`; its next start says so.
pub(super) fn end_in_worktree(
    tx: &Transaction,
    worktree: &str,
    now: &str,
) -> Result<usize, String> {
    tx.execute(
        "UPDATE run SET ended = ?2 WHERE worktree = ?1 AND ended IS NULL",
        rusqlite::params![worktree, now],
    )
    .map_err(err)
}

/// `(uid, title)` of the `running` chunks, outside `except`, whose run is live in a worktree —
/// the ones `end_in_worktree` is about to orphan. Read before the UPDATE, same transaction.
pub(super) fn orphans_in_worktree(
    tx: &Transaction,
    worktree: &str,
    except: &[String],
) -> Result<Vec<(String, String)>, String> {
    let mut q = tx
        .prepare(
            "SELECT DISTINCT c.uid, c.title FROM run r JOIN chunk c ON c.id = r.chunk
             WHERE r.worktree = ?1 AND r.ended IS NULL AND c.state = 'running' ORDER BY r.id",
        )
        .map_err(err)?;
    let rows = q
        .query_map([worktree], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(err)?;
    let all = rows.collect::<Result<Vec<_>, _>>().map_err(err)?;
    Ok(all
        .into_iter()
        .filter(|(u, _)| !except.contains(u))
        .collect())
}

/// Start's worktree step: name the chunks about to be orphaned, then end every live run there.
pub(super) fn end_here(
    tx: &Transaction,
    worktree: Option<&str>,
    ms: &[String],
    now: &str,
) -> Result<Vec<(String, String)>, String> {
    let Some(w) = worktree else {
        return Ok(Vec::new());
    };
    let orphaned = orphans_in_worktree(tx, w, ms)?;
    end_in_worktree(tx, w, now)?;
    Ok(orphaned)
}

/// A pair joins the partner's seat (01M5PE238): the caller's own seat is an
/// agent that left, so its live runs end. Same seat both sides: nothing to
/// free — ending it would kill the partner run just joined.
pub(super) fn end_caller_seat(
    tx: &Transaction,
    pair: bool,
    caller: Option<&str>,
    seat: Option<&str>,
    now: &str,
) -> Result<(), String> {
    if pair
        && let Some(cw) = caller
        && Some(cw) != seat
    {
        end_in_worktree(tx, cw, now)?;
    }
    Ok(())
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

/// Another live, non-stalled run in `worktree` (outside the chunks in `ms` and the run `except`,
/// a pair's own R): two agents there overwrite each other's files, so a start refuses —
/// the one refusal left of the old overlap gate (issue 01M6KHWF). `(uid, run)` each; a
/// stalled run is not listed, `end_here` still ends it as before. A run of the caller's own
/// session is not listed either (it left its work: `end_here` ends it), and neither is one
/// where either session is unknown — only two known, different sessions refuse.
pub(super) fn live_in_worktree(
    tx: &Transaction,
    worktree: &str,
    ms: &[String],
    except: Option<&str>,
    session: Option<&str>,
    stale: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut q = tx
        .prepare(
            "SELECT c.uid, r.start, r.session FROM run r JOIN chunk c ON c.id = r.chunk
             WHERE r.worktree = ?1 AND r.ended IS NULL AND COALESCE(r.last_seen, r.started) >= ?2
             ORDER BY r.id",
        )
        .map_err(err)?;
    let rows = q
        .query_map([worktree, stale], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    fn known(s: Option<&str>) -> Option<&str> {
        s.filter(|s| !s.is_empty())
    }
    Ok(rows
        .into_iter()
        .filter(|(u, run, _)| !ms.contains(u) && Some(run.as_str()) != except)
        .filter(|(_, _, theirs)| {
            matches!((known(session), known(theirs.as_deref())), (Some(a), Some(b)) if a != b)
        })
        .map(|(u, run, _)| (u, run))
        .collect())
}
