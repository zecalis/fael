//! The ledger issues a chunk fixes (PLAN-fael-board l2, decision 01M4NCWEQ): `chunk add
//! --issue` links them, the brief prints them, the board shows each issue's chunk. The ids
//! come checked from the CLI (the ledger is not in plans.db); fael never closes an issue.

use super::store::{Store, err};
use rusqlite::{Connection, params};
use std::collections::HashMap;

pub(super) fn link(c: &Connection, chunk: i64, ids: &[String]) -> Result<(), String> {
    for id in ids {
        c.execute(
            "INSERT OR IGNORE INTO chunk_issue(chunk, row_id) VALUES (?1, ?2)",
            params![chunk, id],
        )
        .map_err(err)?;
    }
    Ok(())
}

/// A merge or split: every new chunk fixes what the old ones did (they keep their links as
/// history; the board shows the newest chunk).
pub(super) fn carry(c: &Connection, old: &[i64], new: &[i64]) -> Result<(), String> {
    for &n in new {
        for &o in old {
            c.execute(
                "INSERT OR IGNORE INTO chunk_issue(chunk, row_id)
                 SELECT ?1, row_id FROM chunk_issue WHERE chunk = ?2",
                params![n, o],
            )
            .map_err(err)?;
        }
    }
    Ok(())
}

/// The issue ids linked to `chunk`, in link order.
pub(super) fn of(c: &Connection, chunk: i64) -> Result<Vec<String>, String> {
    c.prepare_cached("SELECT row_id FROM chunk_issue WHERE chunk = ?1 ORDER BY rowid")
        .map_err(err)?
        .query_map([chunk], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}

/// A linked chunk as the board shows it next to its issue.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Fixer {
    pub uid: String,
    pub state: String,
}

impl Store {
    /// issue id → the newest chunk linked to it (any plan, any state).
    pub fn fixers(&self) -> Result<HashMap<String, Fixer>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT i.row_id, c.uid, c.state FROM chunk_issue i JOIN chunk c ON c.id = i.chunk
                 ORDER BY c.id",
            )
            .map_err(err)?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    Fixer {
                        uid: r.get(1)?,
                        state: r.get(2)?,
                    },
                ))
            })
            .map_err(err)?;
        // later rows (newer chunks) overwrite earlier ones
        rows.collect::<Result<_, _>>().map_err(err)
    }
}
