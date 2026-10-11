//! What `chunk apply` does to the graph around a merge or split: the new chunks take the old
//! ones' `after` and `pair` edges, and the old ones become `replaced`.

use super::State;
use super::apply::Src;
use super::chunk::{add_after, set};
use super::store::err;
use rusqlite::{Transaction, params};

fn ids(tx: &Transaction, sql: &str, old: &[i64]) -> Result<Vec<i64>, String> {
    let list = old.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let sql = sql.replace("OLD", &list);
    tx.prepare(&sql)
        .map_err(err)?
        .query_map([], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}

/// The new chunks take the old ones' place in the graph: each inherits their `after`,
/// whatever waited on them waits on every new one, and a merge keeps their pairs (a split
/// drops them: which part pairs is the planner's call). Returns the pairs dropped.
pub(super) fn inherit(tx: &Transaction, old: &[i64], new: &[i64]) -> Result<usize, String> {
    let deps = ids(
        tx,
        "SELECT DISTINCT dst FROM edge WHERE kind = 'after' AND src IN (OLD)
         AND dst IS NOT NULL AND dst NOT IN (OLD)",
        old,
    )?;
    let waiting = ids(
        tx,
        "SELECT DISTINCT src FROM edge WHERE kind = 'after' AND dst IN (OLD) AND src NOT IN (OLD)",
        old,
    )?;
    let partners = ids(
        tx,
        "SELECT dst FROM edge WHERE kind = 'pair' AND src IN (OLD) AND dst NOT IN (OLD)
         UNION SELECT src FROM edge WHERE kind = 'pair' AND dst IN (OLD) AND src NOT IN (OLD)",
        old,
    )?;
    for &n in new {
        for &d in &deps {
            add_after(tx, n, d)?;
        }
        for &w in &waiting {
            add_after(tx, w, n)?;
        }
        if new.len() == 1 {
            for &p in &partners {
                tx.execute(
                    "INSERT INTO edge(src, dst, kind) VALUES (?1, ?2, 'pair')",
                    params![n, p],
                )
                .map_err(err)?;
            }
        }
    }
    super::issue::carry(tx, old, new)?;
    let list = old.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    tx.execute_batch(&format!(
        "DELETE FROM edge WHERE kind = 'after' AND dst IN ({list}) AND src NOT IN ({list});
         DELETE FROM edge WHERE kind = 'pair' AND (src IN ({list}) OR dst IN ({list}));"
    ))
    .map_err(err)?;
    Ok(if new.len() == 1 { 0 } else { partners.len() })
}

/// The old chunks → `replaced`, one `replaced_by` edge per new chunk.
pub(super) fn replace(
    tx: &Transaction,
    old: &[Src],
    new: &[(i64, String)],
    now: &str,
) -> Result<(), String> {
    let names: Vec<&str> = new.iter().map(|(_, u)| u.as_str()).collect();
    let verb = if new.len() == 1 { "merged" } else { "split" };
    for o in old {
        for (n, _) in new {
            tx.execute(
                "INSERT INTO edge(src, dst, kind) VALUES (?1, ?2, 'replaced_by')",
                params![o.row.id, n],
            )
            .map_err(err)?;
        }
        let text = format!("{verb} into {}", names.join(", "));
        set(tx, &o.row, State::Replaced, "planner", Some(&text), now)?;
    }
    Ok(())
}
