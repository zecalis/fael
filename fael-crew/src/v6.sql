-- A v6 db's schema exactly as fael's own plans.db had it on 2026-10-12 (columns added by
-- ALTER where the code added them, the contract table from fael-parallel), with made-up rows
-- that fill every column epoch v8 drops.
CREATE TABLE plan(
  id INTEGER PRIMARY KEY,
  app TEXT NOT NULL,
  name TEXT NOT NULL,
  title TEXT NOT NULL,
  area TEXT NOT NULL DEFAULT 'dev',
  kind TEXT,
  state TEXT NOT NULL CHECK(state IN ('active','blocked','parked','done')),
  rank INTEGER,
  doc TEXT,
  spec TEXT,
  source TEXT NOT NULL,
  -- who owns the chunk list: the md file until the plan's cutover, then the db
  truth TEXT NOT NULL DEFAULT 'md' CHECK(truth IN ('md','db')),
  -- the paths every brief points at, one per line (`SPEC-x.md#block` allowed)
  refs TEXT, repo TEXT,
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
  approved_client TEXT, client_hint TEXT, reads TEXT, wait_options TEXT,
  UNIQUE(plan, seq));
CREATE TABLE edge(
  src INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  dst INTEGER REFERENCES chunk(id) ON DELETE SET NULL,
  ref TEXT,
  kind TEXT NOT NULL CHECK(kind IN ('after','pair','replaced_by')));
CREATE TABLE run(
  id INTEGER PRIMARY KEY,
  start TEXT NOT NULL,
  chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  client TEXT, session TEXT, model TEXT, worktree TEXT, branch TEXT, pr INTEGER, out TEXT,
  started TEXT NOT NULL, last_seen TEXT, ended TEXT, cost REAL,
  UNIQUE(start, chunk));
CREATE TABLE event(
  id INTEGER PRIMARY KEY,
  chunk_uid TEXT NOT NULL,
  at TEXT NOT NULL,
  by TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  text TEXT, plan INTEGER, batch TEXT);
CREATE TABLE tr(
  key TEXT NOT NULL, lang TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY(key, lang));
CREATE INDEX edge_src ON edge(src);
CREATE INDEX edge_dst ON edge(dst);
CREATE INDEX run_chunk ON run(chunk);
CREATE INDEX event_chunk ON event(chunk_uid);
CREATE INDEX event_plan ON event(plan, id);
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE contract(
       id TEXT PRIMARY KEY,
       from_chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
       to_chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
       want TEXT NOT NULL,
       returns TEXT NOT NULL,
       check_cmd TEXT,
       state TEXT NOT NULL, reason TEXT);
CREATE INDEX contract_from ON contract(from_chunk);
CREATE INDEX contract_to ON contract(to_chunk);
CREATE TABLE chunk_issue(
           chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
           row_id TEXT NOT NULL,
           PRIMARY KEY(chunk, row_id));
CREATE TABLE section(
       id INTEGER PRIMARY KEY,
       plan INTEGER NOT NULL REFERENCES plan(id) ON DELETE CASCADE,
       key TEXT NOT NULL,
       title TEXT NOT NULL,
       body TEXT NOT NULL DEFAULT '',
       seq INTEGER NOT NULL,
       UNIQUE(plan, key));
INSERT INTO plan VALUES
  (1, '', 'app', 'The app', 'dev', 'unit', 'active', 3, '## 1. Goal
ship', 'SPEC-app.md', 'plan/PLAN-app.md', 'db', 'SPEC-app.md#a', '/code/app'),
  (2, '', 'inbox', 'inbox', 'dev', NULL, 'active', NULL, NULL, NULL, '', 'db', NULL, NULL);
INSERT INTO chunk(id, uid, plan, seq, label, title, state, brief, size, model_hint, scope,
                  due, pin, approved_at, approved_model, approved_client, client_hint, reads,
                  wait_options)
  VALUES (1, '01M4AAAAAAAAAAAAAAAAAAAAAA', 1, 1024, 'a', 'a', 'done', 'do a', 'S', 'opus',
          'src/', '2026-10-20', 1, '2026-10-01T00:00:00Z', 'opus', 'claude', 'claude', '1', NULL),
         (2, '01M4BBBBBBBBBBBBBBBBBBBBBB', 1, 2048, 'b', 'b', 'open', 'do b', NULL, NULL, NULL,
          NULL, NULL, NULL, NULL, NULL, NULL, NULL, '["yes","no"]');
INSERT INTO edge VALUES (2, 1, 'section:1', 'after');
INSERT INTO run(id, start, chunk, client, branch, pr, out, started, ended, cost)
  VALUES (1, 'R1', 1, 'claude', 'feat/a', 7, '/out/a', '2026-10-01T00:00:00Z',
          '2026-10-01T01:00:00Z', 0.5);
INSERT INTO event(chunk_uid, at, by, to_state, text, plan)
  VALUES ('01M4AAAAAAAAAAAAAAAAAAAAAA', '2026-10-01T01:00:00Z', 'agent', 'done', 'shipped', 1);
INSERT INTO tr VALUES ('k', 'th', 'x');
INSERT INTO meta VALUES ('db_id', '01M4DBDBDBDBDBDBDBDBDBDBDB'), ('reads', '6');
INSERT INTO contract VALUES ('c1', 1, 2, 'w', 'r', NULL, 'open', NULL);
INSERT INTO chunk_issue VALUES (2, '01M4ISSUEISSUEISSUEISSUEIS');
INSERT INTO section(plan, key, title, body, seq) VALUES (1, '1', '1. Goal', 'ship', 1024);
PRAGMA user_version = 6;
