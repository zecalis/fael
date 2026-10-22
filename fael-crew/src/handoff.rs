//! The hand-offs a brief prints: of the chunks it is `after`, and of every done chunk that
//! touched a file under this chunk's scope since the brief was last written (issue 01M5FMHQX).
//! Deterministic: the db only, no git, no gh.

use super::overlap;
use super::store::err;
use fael_core::under;
use rusqlite::{Transaction, params};

/// `(title, handoff, touched)`: one done / review word and the files its done claimed.
pub type Word = (String, String, String);

/// At most this many chunks by file; the rest are counted, not printed.
const CAP: usize = 5;

/// The done / review word of each chunk `id` is after.
pub(super) fn after(tx: &Transaction, id: i64) -> Result<Vec<Word>, String> {
    let mut q = tx
        .prepare(
            "SELECT c.title, e.text, COALESCE(c.touched, '') FROM edge g
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

/// Done chunks (any plan of this db, not already printed through `after`) whose `touched`
/// meets `scope` and whose done is newer than the last edit of chunk `id`'s brief (its
/// creation when never edited): ranked (most touched files under the scope, same plan, deepest scope, newest), `CAP` of them, and how many were left out.
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
            "SELECT c.title, e.text, COALESCE(c.touched, ''), e.id,
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

#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;
