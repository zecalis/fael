//! `plans.db` — SQLite in WAL mode beside the journal (`<git-common-dir>/fael/`), so every
//! worktree of a clone shares it. Schema per SPEC-fael-board §7; `user_version` carries the
//! schema version and an older db upgrades on open (release gate: no manual step).

use super::md::PlanMd;
use super::schema::{SCHEMA, VERSION, v1_to_v2};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;
use std::time::Duration;

pub struct Store {
    pub(super) conn: Connection,
}

/// One plan file to import: `app` = the `.fapony/`'s folder relative to the repo root
/// (`""` at the root, `apps/vela`), `dir` = `plan` | `parked` | `done`.
pub struct Import {
    pub app: String,
    pub dir: String,
    pub source: String,
    pub md: PlanMd,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub plans: usize,
    pub chunks: usize,
    /// unknown checkbox lines, imported as `draft` (not startable)
    pub drafts: usize,
    /// `app/plan label → ref` for every `(after …)` that names no chunk
    pub unresolved: Vec<String>,
    /// `app/plan label-or-title` for every chunk whose old uid could not be told apart
    /// (SPEC §7): it got a new uid
    pub ambiguous: Vec<String>,
    /// plans whose chunks the db owns (`truth = db`): plan fields refreshed, chunks untouched
    pub db_plans: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub id: i64,
    pub app: String,
    pub name: String,
    pub title: String,
    pub area: String,
    pub state: String,
    pub truth: String,
    /// the md path, repo-relative (`""` for `inbox`)
    pub source: String,
}

pub(super) fn err(e: rusqlite::Error) -> String {
    format!("plans.db: {e}")
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        Store::init(Connection::open(path).map_err(err)?)
    }

    pub fn open_in_memory() -> Result<Store, String> {
        Store::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<Store, String> {
        // 8 agents write at once: wait for the writer instead of failing
        let wait = Duration::from_secs(10);
        conn.busy_timeout(wait).map_err(err)?;
        // the switch to WAL skips the busy handler: a second first-open gets BUSY at once
        // while the first one switches, so it retries within the same budget
        let t = std::time::Instant::now();
        while let Err(e) = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
        {
            if e.sqlite_error_code() != Some(rusqlite::ErrorCode::DatabaseBusy)
                || t.elapsed() > wait
            {
                return Err(err(e));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        // off while a step renames and drops tables (a transaction cannot change it)
        conn.pragma_update(None, "foreign_keys", false)
            .map_err(err)?;
        let mut s = Store { conn };
        s.migrate()?;
        s.conn
            .pragma_update(None, "foreign_keys", true)
            .map_err(err)?;
        Ok(s)
    }

    fn migrate(&mut self) -> Result<(), String> {
        // the common open takes no write lock: every hook and command opens the db
        let v: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        if v == VERSION {
            return Ok(());
        }
        // re-read under the write lock: two first opens race, one creates
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let v: i32 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        if v > VERSION {
            return Err(format!(
                "plans.db is schema v{v}, this fael knows v{VERSION} — run `fael upgrade`"
            ));
        }
        match v {
            0 => tx.execute_batch(SCHEMA).map_err(err)?,
            1 => v1_to_v2(&tx)?,
            _ => return Ok(()),
        }
        tx.pragma_update(None, "user_version", VERSION)
            .map_err(err)?;
        tx.commit().map_err(err)
    }

    pub fn plans(&self) -> Result<Vec<PlanRow>, String> {
        let mut q = self
            .conn
            .prepare("SELECT id, app, name, title, area, state, truth, source FROM plan ORDER BY app, name")
            .map_err(err)?;
        q.query_map([], |r| {
            Ok(PlanRow {
                id: r.get(0)?,
                app: r.get(1)?,
                name: r.get(2)?,
                title: r.get(3)?,
                area: r.get(4)?,
                state: r.get(5)?,
                truth: r.get(6)?,
                source: r.get(7)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
