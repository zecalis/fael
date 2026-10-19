//! `fael.db` — SQLite in WAL mode beside the journal (`<git-common-dir>/fael/`), so every
//! worktree of a clone shares it. Schema per SPEC-fael-board §7; `user_version` carries the
//! breaking epoch, an older db upgrades on open (release gate: no manual step) and every
//! open adds the tables and columns it lacks.

use super::backup::backup;
use super::schema::{
    COLUMNS, SCHEMA, TABLES, VERSION, missing, v1_to_v2, v2_to_v3, v3_to_v4, v4_to_v5, v5_to_v6,
    v6_fill, v6_to_v8,
};
use rusqlite::{Connection, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::Duration;

type Adds = &'static [(&'static str, &'static str)];
type Cols = &'static [(&'static str, &'static str, &'static str)];

pub struct Store {
    pub(super) conn: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub id: i64,
    pub app: String,
    pub name: String,
    pub title: String,
    pub area: String,
    pub state: String,
    /// the umbrella plan's name; `None` = top level
    pub parent: Option<String>,
}

pub(super) fn err(e: rusqlite::Error) -> String {
    format!("fael.db: {e}")
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        Store::init(Connection::open(path).map_err(err)?)
    }

    /// SQLite's `PRAGMA data_version`: moves when another connection commits, so a held
    /// reader (`fael board --watch`) sees a write without writing or rereading.
    pub fn data_version(&self) -> Result<i64, String> {
        self.conn
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .map_err(err)
    }

    pub fn open_in_memory() -> Result<Store, String> {
        Store::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<Store, String> {
        Store::init_with(conn, TABLES, COLUMNS)
    }

    /// `init` with the additions a build carries: a test plays a build with other ones.
    fn init_with(conn: Connection, tables: Adds, columns: Cols) -> Result<Store, String> {
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
        s.migrate(tables, columns)?;
        s.conn
            .pragma_update(None, "foreign_keys", true)
            .map_err(err)?;
        super::secret::guard(&s.conn)?;
        Ok(s)
    }

    fn migrate(&mut self, tables: Adds, columns: Cols) -> Result<(), String> {
        // the common open takes no write lock: every hook and command opens the db
        let v: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        if v > VERSION {
            return Err(newer(v));
        }
        if v == VERSION
            && missing(&self.conn, tables, columns)
                .map_err(err)?
                .is_empty()
        {
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
            return Err(newer(v));
        }
        // an epoch step, not a fresh db or an addition: copy the old one first (no path = in memory)
        if (1..VERSION).contains(&v)
            && let Some(db) = tx.path().filter(|p| !p.is_empty())
        {
            backup(Path::new(db), v)?;
        }
        type Step = fn(&rusqlite::Transaction) -> Result<(), String>;
        let steps: [Step; 5] = [v2_to_v3, v3_to_v4, v4_to_v5, v5_to_v6, v6_to_v8];
        match v {
            0 => tx.execute_batch(SCHEMA).map_err(err)?,
            // v1 creates the current SCHEMA; each later version runs the steps after it
            1 => v1_to_v2(&tx)?,
            2..=6 => {
                for step in &steps[v as usize - 2..] {
                    step(&tx)?;
                }
            }
            _ => {}
        }
        if v < VERSION {
            v6_fill(&tx)?;
            tx.pragma_update(None, "user_version", VERSION)
                .map_err(err)?;
        }
        // re-read under the lock: another open may have added them meanwhile
        tx.execute_batch(&missing(&tx, tables, columns).map_err(err)?)
            .map_err(err)?;
        tx.commit().map_err(err)
    }

    pub fn plans(&self) -> Result<Vec<PlanRow>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT id, app, name, title, area, state, parent FROM plan ORDER BY app, name",
            )
            .map_err(err)?;
        q.query_map([], |r| {
            Ok(PlanRow {
                id: r.get(0)?,
                app: r.get(1)?,
                name: r.get(2)?,
                title: r.get(3)?,
                area: r.get(4)?,
                state: r.get(5)?,
                parent: r.get(6)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }
}

/// One run for the stats flow join: stored RFC 3339 times, the PR number
/// (`done --pr N`), its branch and its chunk's uid. The caller converts the
/// times to ms — the store never parses them.
#[derive(Debug, Clone)]
pub struct StatsRun {
    pub started: String,
    pub ended: Option<String>,
    pub pr: Option<i64>,
    pub branch: Option<String>,
    pub chunk: String,
}

impl Store {
    /// Every run with its chunk's uid, oldest first — the flow spans read
    /// `started`/`ended`/`pr`, throughput reads `ended`.
    pub fn stats_runs(&self) -> Result<Vec<StatsRun>, String> {
        let mut q = self
            .conn
            .prepare(
                "SELECT r.started, r.ended, r.pr, r.branch, c.uid
                 FROM run r JOIN chunk c ON c.id = r.chunk ORDER BY r.id",
            )
            .map_err(err)?;
        q.query_map([], |r| {
            Ok(StatsRun {
                started: r.get(0)?,
                ended: r.get(1)?,
                pr: r.get(2)?,
                branch: r.get(3)?,
                chunk: r.get(4)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }

    /// Is `tok` a chunk uid, or the prefix of one? Ledger ids share the ULID shape.
    pub fn is_chunk(&self, tok: &str) -> bool {
        self.conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM chunk WHERE uid LIKE ?1 || '%')",
                [tok.to_uppercase()],
                |r| r.get(0),
            )
            .unwrap_or(false)
    }

    /// Where a chunk the board does not list sits: `(chunk_state, plan, plan_state)`
    /// for its uid — a dropped chunk, or a chunk whose plan is parked or done.
    pub fn chunk_place(&self, uid: &str) -> Result<Option<(String, String, String)>, String> {
        self.conn
            .query_row(
                "SELECT c.state, CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END, p.state
                 FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.uid = ?1",
                [uid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(super::store::err(e)),
            })
    }
}

/// `<dir>/fael.db`, the clone's db. Epoch v8 named it for the product: a `plans.db` there
/// (and a `-wal`/`-shm` left) becomes `fael.db` on the first call, and a stub `plans.db`
/// at user_version [`STUB`] stays behind so a fael from before v8 refuses it ("schema v…")
/// instead of making an empty db. A rename that fails leaves `plans.db` the db (it upgrades
/// in place) and the next call tries again.
pub fn db_at(dir: &Path) -> PathBuf {
    let (path, old) = (dir.join("fael.db"), dir.join("plans.db"));
    if path.exists() || rename(&old, &path).is_ok() {
        return path;
    }
    old
}

/// The hard link is the lock: of two first opens, one links, the other finds `fael.db`.
pub(super) fn rename(old: &Path, new: &Path) -> Result<(), String> {
    let e = |e: String| format!("plans.db → fael.db refused: {e}; nothing changed");
    let Ok(c) = Connection::open_with_flags(old, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
    else {
        return Ok(());
    };
    let v: i32 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|x| e(x.to_string()))?;
    if v > VERSION {
        return Ok(()); // the stub itself: fael.db was removed, a fresh one starts
    }
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .map_err(|x| e(x.to_string()))?;
    drop(c);
    if let Err(x) = std::fs::hard_link(old, new) {
        return match x.kind() {
            std::io::ErrorKind::AlreadyExists => Ok(()),
            _ => Err(e(x.to_string())),
        };
    }
    for side in ["-wal", "-shm"] {
        let from = format!("{}{side}", old.display());
        let _ = std::fs::rename(&from, format!("{}{side}", new.display()));
    }
    let tmp = old.with_extension(format!("stub.{}", crate::ulid()));
    Connection::open(&tmp)
        .and_then(|c| c.pragma_update(None, "user_version", STUB))
        .map_err(|x| e(x.to_string()))?;
    std::fs::rename(&tmp, old).map_err(|x| e(x.to_string()))
}

/// The stub `plans.db`'s user_version: past any epoch a fael will ever reach.
pub(super) const STUB: i32 = 1_000_000;

/// A db past this fael's epoch: a breaking change it cannot read. The error names the upgrade.
fn newer(v: i32) -> String {
    // a dev build is upgraded by building a newer main, not by the release channel
    let fix = match crate::GIT {
        Some(g) => format!(
            "this fael (dev build {g}) knows v{VERSION} — use a fael built from a newer main: `cargo install --path fael --locked`"
        ),
        None => format!("this fael knows v{VERSION} — run `fael upgrade`"),
    };
    format!("fael.db is schema v{v}, {fix}")
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
