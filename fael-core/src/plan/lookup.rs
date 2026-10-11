//! Which chunk a command names — its uid, or a label (`b10`) — and the rule a new label
//! meets: one word, not uid-shaped, free in its plan (issue 01M4N4CVK).

use super::store::err;
use rusqlite::{OptionalExtension, Transaction, params};

pub(super) type Hit = (i64, String, String, String, String, String);

/// `(id, uid, state, truth, plan, source)` of the chunk `key` names: its uid, else a label
/// only one chunk carries (a label is unique in its plan, not across plans). None = no chunk.
pub(super) fn lookup(tx: &Transaction, key: &str) -> Result<Option<Hit>, String> {
    let mut q = tx
        .prepare(
            "SELECT c.id, c.uid, c.state, p.truth, p.name, p.source
             FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.uid = ?1 OR c.label = ?1
             ORDER BY c.uid = ?1 DESC LIMIT 2",
        )
        .map_err(err)?;
    let hits: Vec<Hit> = q
        .query_map([key], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    match hits.as_slice() {
        [h, ..] if h.1 == key => Ok(Some(h.clone())),
        [h] => Ok(Some(h.clone())),
        [] => Ok(None),
        _ => Err(format!(
            "rejected: label {key} names chunks in more than one plan — use the uid"
        )),
    }
}

/// A label is one word, not uid-shaped, and free in its plan (`id` = the chunk keeping it).
pub(super) fn label(tx: &Transaction, plan: i64, id: i64, l: &str) -> Result<(), String> {
    if l.is_empty() || l.contains(char::is_whitespace) || crate::looks_like_id(l) {
        return Err(format!(
            "rejected: --label {l:?} — one short word like b10, not uid-shaped"
        ));
    }
    let taken: Option<String> = tx
        .query_row(
            "SELECT uid FROM chunk WHERE plan = ?1 AND label = ?2 AND id <> ?3",
            params![plan, l, id],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    match taken {
        Some(u) => Err(format!(
            "rejected: label {l} is taken by chunk {u} in this plan"
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "lookup_tests.rs"]
mod tests;
