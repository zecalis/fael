//! The plans.db schema (SPEC-fael-board §7) and the step from each older `user_version`.
//! Every step runs inside the caller's `BEGIN IMMEDIATE`, so a failure rolls back to the
//! old version and the next open retries.

use rusqlite::Transaction;

pub(super) const VERSION: i32 = 2;

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
  goal TEXT,
  spec TEXT,
  source TEXT NOT NULL,
  -- who owns the chunk list: the md file until the plan's cutover, then the db
  truth TEXT NOT NULL DEFAULT 'md' CHECK(truth IN ('md','db')),
  -- the paths every brief points at, one per line (`SPEC-x.md#block` allowed)
  refs TEXT,
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
  UNIQUE(start, chunk));
-- append-only history of every transition, keyed on uid so it outlives a re-import
CREATE TABLE event(
  id INTEGER PRIMARY KEY,
  chunk_uid TEXT NOT NULL,
  at TEXT NOT NULL,
  by TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  text TEXT);
CREATE TABLE IF NOT EXISTS tr(
  key TEXT NOT NULL, lang TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY(key, lang));
CREATE INDEX edge_src ON edge(src);
CREATE INDEX edge_dst ON edge(dst);
CREATE INDEX run_chunk ON run(chunk);
CREATE INDEX event_chunk ON event(chunk_uid);
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
        "INSERT INTO plan(id, app, name, title, area, kind, state, rank, goal, spec, source)
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

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
