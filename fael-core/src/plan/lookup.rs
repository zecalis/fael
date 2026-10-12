//! Which chunk a command names — its uid, a unique uid prefix of 8+ chars (like ledger ids),
//! or a label (`b10`) — and the rule a new label meets: one word, not uid-shaped, free in its
//! plan (issue 01M4N4CVK).

use super::store::err;
use rusqlite::{OptionalExtension, Transaction, params};

pub(super) type Hit = (i64, String, String);

/// `(id, uid, state)` of the chunk `key` names: its uid, else a label
/// only one chunk carries (a label is unique in its plan, not across plans), else the one
/// uid that starts with it (8+ chars, any case). None = no chunk.
pub(super) fn lookup(tx: &Transaction, key: &str) -> Result<Option<Hit>, String> {
    let exact = hits(
        tx,
        "c.uid = ?1 OR c.label = ?1 ORDER BY c.uid = ?1 DESC LIMIT 2",
        key,
    )?;
    match exact.as_slice() {
        [h, ..] if h.1 == key => return Ok(Some(h.clone())),
        [h] => return Ok(Some(h.clone())),
        [] => {}
        _ => {
            return Err(format!(
                "rejected: label {key} names more than one chunk — use the uid"
            ));
        }
    }
    if !key.starts_with('0') || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Ok(None);
    }
    if key.len() < 8 {
        return Err(format!(
            "rejected: uid prefix {key} is too short — use 8+ chars of the uid"
        ));
    }
    let p = hits(
        tx,
        "c.uid LIKE ?1 || '%' ORDER BY c.uid",
        &key.to_uppercase(),
    )?;
    match p.as_slice() {
        [] => Ok(None),
        [h] => Ok(Some(h.clone())),
        many => Err(format!(
            "rejected: uid prefix {key} names {} chunks — use more of the uid: {}",
            many.len(),
            many.iter()
                .map(|h| h.1.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn hits(tx: &Transaction, cond: &str, key: &str) -> Result<Vec<Hit>, String> {
    let sql = format!("SELECT c.id, c.uid, c.state FROM chunk c WHERE {cond}");
    let mut q = tx.prepare(&sql).map_err(err)?;
    q.query_map([key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
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
