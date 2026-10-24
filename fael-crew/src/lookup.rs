//! Which chunk a command names — its uid, a unique uid prefix of 8+ chars (like ledger ids),
//! or a label (`b10`) — and the rule a new label meets: one word, not uid-shaped, free in its
//! plan (issue 01M4N4CVK).

use super::store::err;
use rusqlite::{OptionalExtension, Transaction, params};

pub(super) type Hit = (i64, String, String);

/// Split a `plan/label` ref at the last `/`: the plan is `name` or `app/name`,
/// the label is the chunk's label (or uid) in that plan. Labels never contain
/// `/` in practice (`show` already reads `p/label` this way); uids never do.
pub(super) fn split_ref(key: &str) -> Option<(&str, &str)> {
    let (p, l) = key.rsplit_once('/')?;
    (!p.is_empty() && !l.is_empty()).then_some((p, l))
}

/// The plans holding `label`, for the ambiguous-label hint.
fn plans_of(tx: &Transaction, label: &str) -> Vec<String> {
    tx.prepare(
        "SELECT DISTINCT CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END
         FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.label = ?1 ORDER BY 1",
    )
    .and_then(|mut q| {
        q.query_map([label], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()
    })
    .unwrap_or_default()
}

/// The hint an ambiguous or misplaced label carries: how to retype it plus the
/// export that lists the uids. Keeps the `try --plan` and `plan export` needles.
fn retype_hint(plans: &[String], label: &str) -> String {
    match plans.first() {
        Some(p) => format!(
            " — try --plan {p} {label} or {p}/{label} (`fael plan export {p}` lists its uids)"
        ),
        None => format!(
            " — try --plan <plan> {label} or <plan>/{label} (`fael plan export <plan>` lists its uids)"
        ),
    }
}

/// `(id, uid, state)` of the chunk `key` names: a `plan/label` through its plan,
/// else its uid, else a label only one chunk carries (a label is unique in its
/// plan, not across plans), else the one uid that starts with it (8+ chars, any
/// case). None = no chunk.
pub(super) fn lookup(tx: &Transaction, key: &str) -> Result<Option<Hit>, String> {
    if let Some((p, l)) = split_ref(key) {
        let uid = uid_in_tx(tx, l, p)?;
        let h = hits(tx, "c.uid = ?1", &uid)?;
        return Ok(h.into_iter().next());
    }
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
            let hint = retype_hint(&plans_of(tx, key), key);
            return Err(format!(
                "rejected: label {key} names more than one chunk — use the uid{hint}"
            ));
        }
    }
    if !key.starts_with('0') || !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Ok(None);
    }
    if key.len() < 8 {
        return Err(format!(
            "rejected: uid prefix {key} is too short — use 8+ chars of the uid; try `fael board` to copy one"
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
            "rejected: uid prefix {key} names {} chunks — use more of the uid; try `fael chunk show <more of the uid>`: {}",
            many.len(),
            many.iter()
                .map(|h| h.1.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The reject for a key that names no chunk: it says what to retype, never a guess.
pub(super) fn no_chunk(key: &str) -> String {
    format!(
        "rejected: no chunk {key} — try `fael board` to copy a uid, or `--plan <plan> <label>` for a label (`fael plan export <plan>` lists its uids)"
    )
}

/// The uid `key` names inside `plan` (`name` or `app/name`): a label in that plan,
/// else what `lookup` finds — refused when that chunk sits in another plan. A key
/// that is already `plan/label` resolves through its own plan, so `apply --plan`
/// keeps working when an op already names one. So `--plan` answers a label two
/// plans share, and checks a uid it is given.
fn uid_in_tx(tx: &Transaction, key: &str, plan: &str) -> Result<String, String> {
    if let Some((p, l)) = split_ref(key) {
        return uid_in_tx(tx, l, p);
    }
    let of = "CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END";
    let named = format!("(p.name = ?2 OR {of} = ?2)");
    let here: Option<String> = tx
        .query_row(
            &format!(
                "SELECT c.uid FROM chunk c JOIN plan p ON p.id = c.plan
                     WHERE c.label = ?1 AND {named}"
            ),
            params![key, plan],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(u) = here {
        return Ok(u);
    }
    let (id, uid, _) = lookup(tx, key)?.ok_or(format!(
        "rejected: no chunk {key} in plan {plan} — try --plan {plan} <label> or {plan}/<label> (`fael plan export {plan}` lists its uids)"
    ))?;
    let (inside, at): (bool, String) = tx
        .query_row(
            &format!(
                "SELECT {named}, {of} FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.id = ?1"
            ),
            params![id, plan],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(err)?;
    match inside {
        true => Ok(uid),
        false => Err(format!(
            "rejected: chunk {uid} is in plan {at}, not {plan} — try --plan {at} or {at}/{key} (`fael plan export {at}` lists its uids)"
        )),
    }
}

impl super::Store {
    /// The uid `key` names inside `plan` (`name` or `app/name`): a label in that plan,
    /// else what `lookup` finds — refused when that chunk sits in another plan. So
    /// `--plan` answers a label two plans share, and checks a uid it is given.
    /// A `plan/label` key resolves through its own plan (issue 01M5KTQY8DTT8APQ2BFGSCSAZY).
    pub fn uid_in(&self, key: &str, plan: &str) -> Result<String, String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        uid_in_tx(&tx, key, plan)
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
    if l.is_empty() || l.contains(char::is_whitespace) || fael_core::looks_like_id(l) {
        return Err(format!(
            "rejected: --label {l:?} — one short word like b10, not uid-shaped; try --label b10"
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
            "rejected: label {l} is taken by chunk {u} in this plan; try --label <another-word>"
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "lookup_tests.rs"]
mod tests;
