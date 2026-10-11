//! The plans.db schema (SPEC-fael-board §7) and the step from each older `user_version`.
//! Every step runs inside the caller's `BEGIN IMMEDIATE`, so a failure rolls back to the
//! old version and the next open retries.

use rusqlite::{Connection, Transaction};

/// The breaking epoch: only a rename, drop or retype bumps it, in a chunk of its own run
/// alone. An addition goes in [`TABLES`] or [`COLUMNS`] instead, so two lanes that each
/// add something open the shared db both ways (issue 01M4MN60).
pub(super) const VERSION: i32 = 6;

/// New tables, `(name, sql)`: the sql runs once when the table is missing — its
/// `CREATE TABLE` and indexes, `IF NOT EXISTS` so a race is harmless.
pub(super) const TABLES: &[(&str, &str)] = &[];

/// New columns, `(table, column, decl)`, added when missing. Nullable or with a DEFAULT,
/// so a fael without the column still inserts rows.
pub(super) const COLUMNS: &[(&str, &str, &str)] = &[];

/// The additions `c` lacks, as one sql batch (`""` = none). A read only: every open
/// checks, and only one that finds something missing takes the write lock.
pub(super) fn missing(
    c: &Connection,
    tables: &[(&str, &str)],
    columns: &[(&str, &str, &str)],
) -> rusqlite::Result<String> {
    let mut sql = String::new();
    for (name, create) in tables {
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |r| r.get(0),
        )?;
        if n == 0 {
            sql += create;
            sql += ";";
        }
    }
    for (table, col, decl) in columns {
        let n: i64 = c.query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
            [table, col],
            |r| r.get(0),
        )?;
        if n == 0 {
            sql += &format!("ALTER TABLE {table} ADD COLUMN {col} {decl};");
        }
    }
    Ok(sql)
}

pub(super) const SCHEMA: &str = "
CREATE TABLE plan(
  id INTEGER PRIMARY KEY,
  app TEXT NOT NULL,
  name TEXT NOT NULL,
  title TEXT NOT NULL,
  area TEXT NOT NULL DEFAULT 'dev',
  kind TEXT,
  state TEXT NOT NULL CHECK(state IN ('active','blocked','parked','done')),
  rank INTEGER,
  -- the plan's Goal/Scope/Done/Constraints text (master-detail: md is export only)
  doc TEXT,
  spec TEXT,
  source TEXT NOT NULL,
  -- who owns the chunk list: the md file until the plan's cutover, then the db
  truth TEXT NOT NULL DEFAULT 'md' CHECK(truth IN ('md','db')),
  -- the paths every brief points at, one per line (`SPEC-x.md#block` allowed)
  refs TEXT,
  -- the checkout the plan's code lives in when it is not this repo (`fael plan repo`)
  repo TEXT,
  UNIQUE(app, name));
CREATE TABLE chunk(
  id INTEGER PRIMARY KEY,
  uid TEXT NOT NULL UNIQUE,
  plan INTEGER NOT NULL REFERENCES plan(id) ON DELETE CASCADE,
  -- order only, steps of 1024 so an insert takes a gap; nothing keys on it
  seq INTEGER NOT NULL,
  label TEXT,
  title TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN
    ('draft','open','running','waiting','review','done','replaced','dropped','parked')),
  brief TEXT NOT NULL DEFAULT '',
  wait_on TEXT,
  wait_text TEXT,
  wait_until TEXT,
  size TEXT,
  model_hint TEXT,
  scope TEXT,
  due TEXT,
  pin INTEGER,
  approved_at TEXT,
  approved_model TEXT,
  approved_client TEXT,
  -- free strings like model_hint: the app owns the list of clients (b6b)
  client_hint TEXT,
  UNIQUE(plan, seq));
-- after: src waits on dst · pair: src starts with dst · replaced_by: src lives on in dst
-- dst NULL = an md `(after …)` that names no chunk: never met, shown blocked on `ref`
CREATE TABLE edge(
  src INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  dst INTEGER REFERENCES chunk(id) ON DELETE SET NULL,
  ref TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('after','pair','replaced_by')));
-- start = R, one `chunk start` (a pair = one row per member, same R)
CREATE TABLE run(
  id INTEGER PRIMARY KEY,
  start TEXT NOT NULL,
  chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  client TEXT, session TEXT, model TEXT, worktree TEXT, branch TEXT, pr INTEGER, out TEXT,
  started TEXT NOT NULL, last_seen TEXT, ended TEXT,
  cost REAL,
  UNIQUE(start, chunk));
-- append-only history of every transition, keyed on uid so it outlives a re-import
CREATE TABLE event(
  id INTEGER PRIMARY KEY,
  chunk_uid TEXT NOT NULL,
  at TEXT NOT NULL,
  by TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  text TEXT,
  -- the plan the row belongs to; MAX(id) of a plan is its rev
  plan INTEGER,
  -- one ulid shared by every row one `chunk apply` wrote
  batch TEXT);
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS tr(
  key TEXT NOT NULL, lang TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY(key, lang));
CREATE INDEX edge_src ON edge(src);
CREATE INDEX edge_dst ON edge(dst);
CREATE INDEX run_chunk ON run(chunk);
CREATE INDEX event_chunk ON event(chunk_uid);
CREATE INDEX event_plan ON event(plan, id);
";

/// v1 (b1) → v2: rename the old tables away, create v2, copy, drop. The caller holds
/// `foreign_keys = OFF` (a pragma a transaction cannot change), so no drop cascades.
pub(super) fn v1_to_v2(tx: &Transaction) -> Result<(), String> {
    let e = |e: rusqlite::Error| format!("plans.db v1 → v2: {e}");
    tx.execute_batch(
        "DROP INDEX chunk_plan; DROP INDEX dep_chunk; DROP INDEX run_chunk;
         ALTER TABLE plan RENAME TO plan_v1;
         ALTER TABLE chunk RENAME TO chunk_v1;
         ALTER TABLE run RENAME TO run_v1;
         ALTER TABLE event RENAME TO event_v1;",
    )
    .map_err(e)?;
    tx.execute_batch(SCHEMA).map_err(e)?;
    tx.execute_batch(
        "INSERT INTO plan(id, app, name, title, area, kind, state, rank, doc, spec, source)
           SELECT id, app, name, title, area, kind, state, rank, goal, spec, source FROM plan_v1;
         INSERT INTO chunk(id, uid, plan, seq, label, title, state, brief, wait_on, wait_text,
                           size, model_hint, scope, due, pin)
           SELECT id, 'v1:' || id, plan, (pos + 1) * 1024, label, title,
                  CASE state WHEN 'merged' THEN 'replaced' ELSE state END,
                  brief, wait_on, wait_text, size, model_hint, scope, due, pin FROM chunk_v1;
         INSERT INTO edge(src, dst, ref, kind) SELECT chunk, after, ref, 'after' FROM dep;
         INSERT INTO edge(src, dst, kind) SELECT a, b, 'pair' FROM pair;
         INSERT INTO edge(src, dst, kind)
           SELECT id, into_chunk, 'replaced_by' FROM chunk_v1 WHERE into_chunk IS NOT NULL;
         INSERT INTO run(id, start, chunk, client, session, model, worktree, branch, pr,
                         started, last_seen, ended)
           SELECT id, 'v1:' || id, chunk, client, session, model, worktree, branch, pr,
                  started, last_seen, ended FROM run_v1;",
    )
    .map_err(e)?;
    for t in ["chunk", "run"] {
        let col = if t == "chunk" { "uid" } else { "start" };
        let ids: Vec<i64> = tx
            .prepare(&format!("SELECT id FROM {t}"))
            .map_err(e)?
            .query_map([], |r| r.get(0))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;
        for id in ids {
            tx.execute(
                &format!("UPDATE {t} SET {col} = ?1 WHERE id = ?2"),
                rusqlite::params![crate::ulid(), id],
            )
            .map_err(e)?;
        }
    }
    tx.execute_batch(
        "INSERT INTO event(id, chunk_uid, at, by, from_state, to_state, text)
           SELECT e.id, COALESCE(c.uid, 'v1:' || e.chunk, 'v1:none'), e.at, e.by, e.from_state,
                  e.to_state, e.text
           FROM event_v1 e LEFT JOIN chunk c ON c.id = e.chunk;
         DROP TABLE dep; DROP TABLE pair; DROP TABLE run_v1; DROP TABLE event_v1;
         DROP TABLE chunk_v1; DROP TABLE plan_v1;",
    )
    .map_err(e)?;
    let broken: i64 = tx
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(e)?;
    if broken > 0 {
        return Err(format!(
            "plans.db v1 → v2 left {broken} dangling reference(s)"
        ));
    }
    Ok(())
}

/// `run.cost` (k1): what the client reported start R cost, in USD.
pub(super) fn v2_to_v3(tx: &Transaction) -> Result<(), String> {
    tx.execute_batch("ALTER TABLE run ADD COLUMN cost REAL;")
        .map_err(|e| format!("plans.db v2 → v3: {e}"))
}

/// `chunk.client_hint`: which client the planner or owner picked, next to `model_hint`.
pub(super) fn v3_to_v4(tx: &Transaction) -> Result<(), String> {
    tx.execute_batch("ALTER TABLE chunk ADD COLUMN client_hint TEXT;")
        .map_err(|e| format!("plans.db v3 → v4: {e}"))
}

/// `plan.repo` (issue 01M4N4CVH): a plan whose code is in another checkout.
pub(super) fn v4_to_v5(tx: &Transaction) -> Result<(), String> {
    tx.execute_batch("ALTER TABLE plan ADD COLUMN repo TEXT;")
        .map_err(|e| format!("plans.db v4 → v5: {e}"))
}

/// Master-detail (decision fael-board:master-detail): `plan.doc`, `event.plan` + `batch`,
/// and `meta` for the db's stable id. `goal` had no reader or writer.
pub(super) fn v5_to_v6(tx: &Transaction) -> Result<(), String> {
    tx.execute_batch(
        "ALTER TABLE plan RENAME COLUMN goal TO doc;
         ALTER TABLE event ADD COLUMN plan INTEGER;
         ALTER TABLE event ADD COLUMN batch TEXT;
         CREATE INDEX event_plan ON event(plan, id);
         CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )
    .map_err(|e| format!("plans.db v5 → v6: {e}"))
}

/// Every migration ends here: `event.plan` for rows written before v6 (a chunk's via its
/// uid, a plan's own via `plan:<app>/<name>`; a vanished chunk's stays NULL) and the
/// `db_id` fael-sync keys on, set once and never changed, and `reads` = [`VERSION`]:
/// fael v0.44 opens a newer db while `reads` is at most its own version.
pub(super) fn v6_fill(tx: &Transaction) -> Result<(), String> {
    tx.execute_batch(
        "UPDATE event SET plan = (SELECT plan FROM chunk WHERE uid = event.chunk_uid)
           WHERE plan IS NULL;
         UPDATE event SET plan = (SELECT id FROM plan p
                                  WHERE 'plan:' || p.app || '/' || p.name = event.chunk_uid)
           WHERE plan IS NULL AND chunk_uid LIKE 'plan:%';",
    )
    .and_then(|_| {
        tx.execute(
            "INSERT OR IGNORE INTO meta(key, value) VALUES ('db_id', ?1)",
            [crate::ulid()],
        )
    })
    .and_then(|_| {
        tx.execute(
            "INSERT OR REPLACE INTO meta(key, value) VALUES ('reads', ?1)",
            [VERSION.to_string()],
        )
    })
    .map(drop)
    .map_err(|e| format!("plans.db v6: {e}"))
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
