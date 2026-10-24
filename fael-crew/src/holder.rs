//! Who holds a start now: the live run, the pair members, the worktree
//! slot and the after-block — the guards `Store::start` checks before it
//! claims anything. Split out of `start.rs` (file-size ratchet).

use super::board_math::under;
use super::ready::{Ready, ready_in};
use super::shared::unshared;
use super::store::err;
use fael_core::OverlapClass;
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

/// One live-run clash: who holds the files, what hit, and the pair-level
/// class (`Exact` = same file, else `Prefix` = dir covers file).
struct Clash {
    uid: String,
    label: String,
    plan: String,
    hits: Vec<String>,
    class: OverlapClass,
    started: String,
    seen: Option<String>,
    stalled: bool,
}

/// Live-run chunks of the same app, outside the pair `ms`, whose scope (less `shared`) shares
/// a path prefix with a member's (the board's `overlap` rule).
fn clashes_in(
    tx: &Transaction,
    ms: &[String],
    shared: &[String],
    stale: &str,
) -> Result<Vec<Clash>, String> {
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
            "SELECT c.uid, COALESCE(c.label, c.title), p.name, c.scope, r.started, r.last_seen
             FROM chunk c JOIN plan p ON p.id = c.plan
             JOIN run r ON r.id = (SELECT x.id FROM run x WHERE x.chunk = c.id
               AND x.ended IS NULL ORDER BY x.started DESC LIMIT 1)
             WHERE c.scope IS NOT NULL AND p.app = ?1",
        )
        .map_err(err)?;
    let rows = q
        .query_map([app], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let mut out = Vec::new();
    for (uid, label, plan, scope, started, seen) in rows {
        if ms.contains(&uid) {
            continue;
        }
        let theirs = paths(scope);
        let hit: Vec<_> = mine
            .iter()
            .filter(|x| theirs.iter().any(|y| under(x, y) || under(y, x)))
            .cloned()
            .collect();
        if hit.is_empty() {
            continue;
        }
        // hits prove the overlap, so the class is always Some
        let class = fael_core::overlap_class(&mine, &theirs).unwrap_or(OverlapClass::Prefix);
        let stalled = seen.as_deref().unwrap_or(&started) < stale;
        out.push(Clash {
            uid,
            label,
            plan,
            hits: hit,
            class,
            started,
            seen,
            stalled,
        });
    }
    Ok(out)
}

/// The class of each live-run clash, by chunk uid — what o2's joint command
/// reads to offer pair (exact) vs lane (prefix). Judges nothing.
#[allow(dead_code)] // o2 reads this; o1's tests pin it meanwhile
pub(super) fn clash_classes(
    tx: &Transaction,
    ms: &[String],
    shared: &[String],
    stale: &str,
) -> Result<Vec<(String, OverlapClass)>, String> {
    Ok(clashes_in(tx, ms, shared, stale)?
        .into_iter()
        .map(|c| (c.uid, c.class))
        .collect())
}

/// Live-run chunks of the same app, outside the pair `ms`, whose scope (less `shared`) shares
/// a path prefix with a member's (the board's `overlap` rule): one `uid (plan/label) [class]: path, … —
/// since T · last seen T` line each, plus `· stalled` when the run is older than `stale` (the
/// board's rule). Judges nothing.
pub(super) fn clashes(
    tx: &Transaction,
    ms: &[String],
    shared: &[String],
    stale: &str,
) -> Result<Vec<String>, String> {
    Ok(clashes_in(tx, ms, shared, stale)?
        .into_iter()
        .map(|c| {
            format!(
                "{} ({}/{}): [{}] {} — since {} · last seen {}{}",
                c.uid,
                c.plan,
                c.label,
                c.class.as_str(),
                c.hits.join(", "),
                c.started,
                c.seen.as_deref().unwrap_or("never"),
                if c.stalled { " · stalled" } else { "" }
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::chunk::tests::{add, here, start};
    use super::super::{Start, Store};
    use super::*;
    use rusqlite::params;

    fn scoped(s: &mut Store, title: &str, scope: &str) -> String {
        let c = add(s, title, Some(title));
        s.conn
            .execute(
                "UPDATE chunk SET scope = ?2 WHERE uid = ?1",
                params![c, scope],
            )
            .unwrap();
        c
    }

    #[test]
    fn clash_lines_name_exact_and_prefix() {
        let mut s = Store::open_in_memory().unwrap();
        let a = scoped(&mut s, "a", "src/");
        let b = scoped(&mut s, "b", "src/x.rs");
        let c = scoped(&mut s, "c", "src/x.rs");
        start(&mut s, &a, "/w1").unwrap();
        let stale = "2026-10-11T09:00:00Z";
        // b sits under running a's dir: prefix, and the line says so
        {
            let tx = s.conn.unchecked_transaction().unwrap();
            let lines = clashes(&tx, std::slice::from_ref(&b), &[], stale).unwrap();
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert!(
                lines[0].contains("[prefix]") && lines[0].contains("src/x.rs"),
                "{}",
                lines[0]
            );
            assert_eq!(
                clash_classes(&tx, std::slice::from_ref(&b), &[], stale).unwrap(),
                vec![(a.clone(), OverlapClass::Prefix)]
            );
            // shared paths are nobody's claim: the same check stays silent
            assert!(
                clashes(&tx, std::slice::from_ref(&b), &["src/".into()], stale)
                    .unwrap()
                    .is_empty()
            );
        }
        // c names the exact file b names: exact, alongside a's prefix
        s.start(
            &c,
            &Start {
                force: true,
                ..Start::default()
            },
            &here("/w2"),
        )
        .unwrap();
        let tx = s.conn.unchecked_transaction().unwrap();
        let mut classes = clash_classes(&tx, std::slice::from_ref(&b), &[], stale).unwrap();
        classes.sort_by(|x, y| x.0.cmp(&y.0));
        // uids sort randomly when two rows share one ms (Windows clock), so
        // the expectation sorts too — creation order is not uid order.
        let mut expected = vec![(a, OverlapClass::Prefix), (c, OverlapClass::Exact)];
        expected.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(classes, expected);
        let lines = clashes(&tx, std::slice::from_ref(&b), &[], stale).unwrap();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("[exact]")), "{lines:?}");
    }
}
