//! The claims a loop of asks is decided by (decision fael-parallel:contract): who started
//! first, and which asks between running chunks lead from the holder back to the asker.

use super::contract::Row;
use super::store::err;
use rusqlite::{OptionalExtension, Transaction};
use std::collections::HashMap;

/// Where the claim of chunk `id` began: its latest run's start (ties by run id).
pub(super) fn claimed(tx: &Transaction, id: i64) -> Result<(String, i64), String> {
    tx.query_row(
        "SELECT started, id FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(err)
    .map(|r| r.unwrap_or_default())
}

/// The asks from `to` back to `from` (`from → to` would close the loop), open or agreed,
/// between running chunks only; `None` = no loop.
pub(super) fn loop_path(tx: &Transaction, from: i64, to: i64) -> Result<Option<Vec<Row>>, String> {
    let mut q = tx
        .prepare(
            "SELECT k.id, k.from_chunk, k.to_chunk, k.state FROM contract k
             JOIN chunk f ON f.id = k.from_chunk JOIN chunk t ON t.id = k.to_chunk
             WHERE k.state IN ('open','agreed') AND f.state = 'running' AND t.state = 'running'",
        )
        .map_err(err)?;
    let edges: Vec<Row> = q
        .query_map([], |r| {
            Ok(Row {
                id: r.get(0)?,
                from: r.get(1)?,
                to: r.get(2)?,
                state: r.get(3)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    // breadth first from `to`; `via[n]` = the ask that first reached chunk n
    let mut via: HashMap<i64, usize> = HashMap::new();
    let mut seen = vec![to];
    let mut i = 0;
    while i < seen.len() && !via.contains_key(&from) {
        let at = seen[i];
        i += 1;
        for (k, e) in edges.iter().enumerate().filter(|(_, e)| e.from == at) {
            if e.to != to && !via.contains_key(&e.to) {
                via.insert(e.to, k);
                seen.push(e.to);
            }
        }
    }
    let mut path = Vec::new();
    let mut at = from;
    while at != to {
        let Some(&k) = via.get(&at) else {
            return Ok(None);
        };
        path.push(k);
        at = edges[k].from;
    }
    Ok(Some(path.into_iter().map(|k| edges[k].clone()).collect()))
}
