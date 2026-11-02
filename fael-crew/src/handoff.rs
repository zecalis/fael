//! The hand-offs a brief prints: of the chunks it is `after`, and of every done chunk that
//! touched a file under this chunk's scope since the brief was last written (issue 01M5FMHQX).
//! Deterministic: the db only, no git, no gh.

use super::overlap;
use super::store::err;
use fael_core::under;
use rusqlite::{OptionalExtension, Transaction, params};

/// `(title, handoff, touched)`: one done / review word and the files its done claimed.
/// From [`by_file`] the title is `<uid> <title>`: the brief prints it as one line.
pub type Word = (String, String, String);

/// At most this many chunks by file; the rest are counted, not printed.
const CAP: usize = 5;

/// One avoid card (l3, issue 01M6CZ0K): a running chunk whose scope collides with
/// mine. Who (uid/title/client) + class exact|prefix + files + where
/// (run worktree/branch) + when (started/last_seen/stalled) + doing (live intent,
/// never guessed) + do (how to start beside it). Db only, no git/gh.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Avoid {
    pub uid: String,
    pub title: String,
    pub client: Option<String>,
    pub class: String,
    pub files: Vec<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub started: String,
    pub last_seen: Option<String>,
    pub stalled: bool,
    pub doing: String,
    #[serde(rename = "do")]
    pub do_cmd: String,
}

/// Live scope clash: running chunks of my app whose scope meets mine.
/// `(uid, title, client, class, files, worktree, branch, started, last_seen, touch_msg)`
/// newest run first. Self is left out; pairs/joints are not filtered here (the board
/// filters them for `overlaps`; an avoid card names every clash so the agent sees it).
type ClashRow = (
    String,
    String,
    Option<String>,
    String,
    Vec<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
);

type DbRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
);

fn clashes(tx: &Transaction, id: i64, mine: &[String], app: &str) -> Result<Vec<ClashRow>, String> {
    let mut q = tx
        .prepare(
            "SELECT c.uid, c.title, r.client, c.scope, r.worktree, r.branch,
                    r.started, r.last_seen, r.touch_msg
             FROM chunk c JOIN plan p ON p.id = c.plan
             JOIN run r ON r.id = (SELECT x.id FROM run x WHERE x.chunk = c.id
               AND x.ended IS NULL ORDER BY x.id DESC LIMIT 1)
             WHERE c.id != ?1 AND p.app = ?2 AND c.scope IS NOT NULL AND c.state = 'running'
             ORDER BY r.id DESC",
        )
        .map_err(err)?;
    let rows: Vec<DbRow> = q
        .query_map(params![id, app], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut out = Vec::new();
    for (uid, title, client, scope, worktree, branch, started, seen, touch_msg) in rows {
        let theirs: Vec<String> = scope
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        if theirs.is_empty() {
            continue;
        }
        let hits: Vec<String> = mine
            .iter()
            .filter(|m| theirs.iter().any(|t| under(m, t) || under(t, m)))
            .cloned()
            .collect();
        if hits.is_empty() {
            continue;
        }
        let class = fael_core::overlap_class(mine, &theirs)
            .map(|c| c.as_str().to_string())
            .unwrap_or_else(|| "prefix".to_string());
        out.push((
            uid, title, client, class, hits, worktree, branch, started, seen, touch_msg,
        ));
    }
    Ok(out)
}

/// What the other side is doing (never guessed): its live `touch --msg`, else its
/// latest `chunk note` since its last start, else its last done/review word,
/// else `nothing handed off yet`.
fn doing(tx: &Transaction, partner: &str, touch_msg: Option<String>) -> Result<String, String> {
    if let Some(m) = touch_msg
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
    {
        return Ok(m);
    }
    let note: Option<String> = tx
        .query_row(
            "SELECT text FROM event WHERE chunk_uid = ?1 AND by = 'note' AND text IS NOT NULL
              AND id > COALESCE((SELECT MAX(id) FROM event WHERE chunk_uid = ?1
                AND to_state = 'running' AND by != 'note'), 0)
             ORDER BY id DESC LIMIT 1",
            [partner],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?
        .flatten();
    if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
        return Ok(n);
    }
    let last: Option<String> = tx
        .query_row(
            "SELECT text FROM event WHERE chunk_uid = ?1 AND by = 'agent'
              AND to_state IN ('done', 'review') AND text IS NOT NULL AND text != ''
             ORDER BY id DESC LIMIT 1",
            [partner],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?
        .flatten();
    Ok(last
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "nothing handed off yet".to_string()))
}

/// The done / review word of each chunk `id` is after.
pub(super) fn after(tx: &Transaction, id: i64) -> Result<Vec<Word>, String> {
    let mut q = tx
        .prepare(
            "SELECT c.title || COALESCE(' (#' || (SELECT pr FROM run WHERE chunk = c.id
                                                    AND pr IS NOT NULL ORDER BY id DESC LIMIT 1) || ')', ''),
                    e.text, COALESCE(c.touched, '') FROM edge g
             JOIN chunk c ON c.id = g.dst
             JOIN event e ON e.id = (SELECT MAX(id) FROM event WHERE chunk_uid = c.uid
                                       AND by = 'agent' AND to_state IN ('done', 'review'))
             WHERE g.src = ?1 AND g.kind = 'after' AND e.text != ''
             ORDER BY c.seq",
        )
        .map_err(err)?;
    q.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}

/// The chunks whose latest done / review hand-off names `id`'s chunk on its `left:` line — the
/// origin of a follow-up, so its brief reads what the origin measured and decided without an
/// `after` edge being drawn (issue 01M6BKHZSP). `<uid> <title>` words, newest first, `CAP` at most.
pub(super) fn left_over(tx: &Transaction, id: i64) -> Result<Vec<Word>, String> {
    let uid: String = tx
        .query_row("SELECT uid FROM chunk WHERE id = ?1", [id], |r| r.get(0))
        .map_err(err)?;
    let mut q = tx
        .prepare(
            "SELECT c.uid || ' ' || c.title, e.text, COALESCE(c.touched, '') FROM chunk c
             JOIN event e ON e.id = (SELECT MAX(id) FROM event WHERE chunk_uid = c.uid
                                       AND by = 'agent' AND to_state IN ('done', 'review'))
             WHERE c.id != ?1 AND e.text LIKE '%left:%'
               AND c.id NOT IN (SELECT dst FROM edge
                                 WHERE src = ?1 AND kind = 'after' AND dst IS NOT NULL)
             ORDER BY e.id DESC",
        )
        .map_err(err)?;
    let all: Vec<Word> = q
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(all
        .into_iter()
        .filter(|(_, text, _)| names_left(text, &uid))
        .take(CAP)
        .collect())
}

/// Does the hand-off's `left:` line name `uid` — by its uid or a prefix of it, as
/// `chunk done --left` took it?
fn names_left(handoff: &str, uid: &str) -> bool {
    handoff
        .lines()
        .find_map(|l| l.trim().strip_prefix("left:"))
        .is_some_and(|v| {
            v.split(',')
                .map(|t| t.trim().to_uppercase())
                .any(|t| t.len() >= 4 && uid.starts_with(&t))
        })
}

/// Done chunks (any plan of this db, not already printed through `after`) whose `touched`
/// meets `scope` and whose done is newer than the last edit of chunk `id`'s brief (its
/// creation when never edited): ranked (most touched files under the scope, same plan,
/// deepest scope, newest), `CAP` of them, and how many were left out.
pub(super) fn by_file(
    tx: &Transaction,
    id: i64,
    scope: Option<&str>,
) -> Result<(Vec<Word>, usize), String> {
    let mine: Vec<String> = scope
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    if mine.is_empty() {
        return Ok((vec![], 0));
    }
    let mut q = tx
        .prepare(
            "SELECT c.uid || ' ' || c.title, e.text, COALESCE(c.touched, ''), e.id,
                    c.plan IS (SELECT plan FROM chunk WHERE id = ?1)
             FROM chunk c
             JOIN event e ON e.id = (SELECT MAX(id) FROM event WHERE chunk_uid = c.uid
                                       AND by = 'agent' AND to_state = 'done')
             WHERE c.state = 'done' AND c.id != ?1 AND c.touched IS NOT NULL AND e.text != ''
               AND c.id NOT IN (SELECT dst FROM edge
                                 WHERE src = ?1 AND kind = 'after' AND dst IS NOT NULL)
               AND e.id > COALESCE(
                     (SELECT MAX(v.id) FROM event v JOIN chunk m ON m.uid = v.chunk_uid
                       WHERE m.id = ?1 AND v.text LIKE 'edited%brief%'),
                     (SELECT MIN(v.id) FROM event v JOIN chunk m ON m.uid = v.chunk_uid
                       WHERE m.id = ?1), 0)
             ORDER BY e.id DESC",
        )
        .map_err(err)?;
    let all: Vec<(Word, i64, bool)> = q
        .query_map(params![id], |r| {
            Ok(((r.get(0)?, r.get(1)?, r.get(2)?), r.get(3)?, r.get(4)?))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    // rank key: (files under the scope, same plan, deepest matching scope, event id)
    let mut hit: Vec<((usize, bool, usize, i64), Word)> = all
        .into_iter()
        .filter_map(|(w, ev, same)| {
            let files: Vec<String> =
                w.2.lines()
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect();
            if !overlap(&mine, &files) {
                return None;
            }
            let under_n = files
                .iter()
                .filter(|f| mine.iter().any(|m| under(f, m)))
                .count();
            let depth = mine
                .iter()
                .filter(|m| files.iter().any(|f| under(f, m)))
                .map(|m| m.trim_end_matches('/').matches('/').count())
                .max()
                .unwrap_or(0);
            Some(((under_n, same, depth, ev), w))
        })
        .collect();
    hit.sort_by_key(|h| std::cmp::Reverse(h.0));
    let more = hit.len().saturating_sub(CAP);
    Ok((hit.into_iter().take(CAP).map(|(_, w)| w).collect(), more))
}

/// Live touches: running chunks (any plan of this db) whose live `run.touched`
/// meets `scope` — what `chunk touch` reported so far, not what `done` claimed.
/// Newest run first (highest run id), capped like [`by_file`]: the rest are counted,
/// not printed. `(title, touched)`: no hand-off text yet — the chunk still runs —
/// so the brief prints this section distinct from the done-claimed one above.
/// Self is left out; nothing else is filtered (an `after` edge only hides done
/// hand-offs, and a lane partner's live files are what the waiter must avoid).
pub(super) fn live(
    tx: &Transaction,
    id: i64,
    scope: Option<&str>,
) -> Result<(Vec<(String, String)>, usize), String> {
    let mine: Vec<String> = scope
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    if mine.is_empty() {
        return Ok((vec![], 0));
    }
    let mut q = tx
        .prepare(
            "SELECT c.title, COALESCE(r.touched, ''), r.id
             FROM chunk c JOIN run r ON r.chunk = c.id
             WHERE c.state = 'running' AND c.id != ?1 AND r.ended IS NULL
               AND r.touched IS NOT NULL
             ORDER BY r.id DESC",
        )
        .map_err(err)?;
    let all: Vec<(String, String)> = q
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut hit: Vec<(String, String)> = all
        .into_iter()
        .filter(|(_, t)| {
            let files: Vec<String> = t
                .lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect();
            !files.is_empty() && overlap(&mine, &files)
        })
        .collect();
    // newest first already (r.id DESC); trim each chunk's files to those under the scope?
    // No: show exactly what the agent reported (never infer, never trim) — the overlap
    // check above already gated on meeting the scope.
    let more = hit.len().saturating_sub(CAP);
    hit.truncate(CAP);
    Ok((hit, more))
}

/// Avoid cards (l3): one per running chunk whose scope collides with mine.
/// Who (uid/title/client) + class + files + where (worktree/branch) + when
/// (started/last_seen/stalled) + doing (live intent, never guessed) + do.
/// `stale` is the board's 30-min line; `my_uid` names the waiter for `do`.
pub(super) fn avoid(
    tx: &Transaction,
    id: i64,
    scope: Option<&str>,
    stale: &str,
) -> Result<(Vec<Avoid>, usize), String> {
    let mine: Vec<String> = scope
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    if mine.is_empty() {
        return Ok((vec![], 0));
    }
    let (my_uid, app): (String, String) = tx
        .query_row(
            "SELECT c.uid, p.app FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(err)?;
    let mut out = Vec::new();
    for (uid, title, client, class, files, worktree, branch, started, seen, touch_msg) in
        clashes(tx, id, &mine, &app)?
    {
        let stalled = seen.as_deref().unwrap_or(&started) < stale;
        let live = doing(tx, &uid, touch_msg)?;
        let follow = format!("fael chunk start {my_uid} --with {uid} --as follow");
        let side = match class.as_str() {
            "exact" => format!("fael chunk start {my_uid} --with {uid} --as pair"),
            _ => format!("fael chunk start {my_uid} --with {uid} --as lane"),
        };
        out.push(Avoid {
            uid: uid.clone(),
            title,
            client,
            class,
            files,
            worktree,
            branch,
            started,
            last_seen: seen,
            stalled,
            doing: live,
            do_cmd: format!(
                "wait (fael chunk wait {my_uid} --on owner \"<why>\") or {side} or {follow}"
            ),
        });
    }
    let more = out.len().saturating_sub(CAP);
    out.truncate(CAP);
    Ok((out, more))
}

#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;
