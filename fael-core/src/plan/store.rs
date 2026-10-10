//! `plans.db` — SQLite in WAL mode beside the journal (`<git-common-dir>/fael/`), so every
//! worktree of a clone shares it. Schema per SPEC-fael-board §7; `user_version` carries the
//! schema version and an older db upgrades on open (release gate: no manual step).

use super::md::{self, PlanMd, Tick};
use rusqlite::{Connection, TransactionBehavior, params};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

const VERSION: i32 = 1;

const SCHEMA: &str = "
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
  UNIQUE(app, name));
CREATE TABLE chunk(
  id INTEGER PRIMARY KEY,
  plan INTEGER NOT NULL REFERENCES plan(id) ON DELETE CASCADE,
  pos INTEGER NOT NULL,
  label TEXT,
  title TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN
    ('draft','open','running','waiting','review','done','merged','dropped','parked')),
  brief TEXT NOT NULL DEFAULT '',
  wait_on TEXT,
  wait_text TEXT,
  size TEXT,
  model_hint TEXT,
  scope TEXT,
  due TEXT,
  pin INTEGER,
  into_chunk INTEGER REFERENCES chunk(id) ON DELETE SET NULL,
  UNIQUE(plan, pos));
-- after NULL = a ref that names no chunk: never met, shown as blocked on `ref`
CREATE TABLE dep(
  chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  after INTEGER REFERENCES chunk(id) ON DELETE SET NULL,
  ref TEXT NOT NULL);
CREATE TABLE pair(
  a INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  b INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE);
CREATE TABLE run(
  id INTEGER PRIMARY KEY,
  chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  client TEXT, session TEXT, model TEXT, worktree TEXT, branch TEXT, pr INTEGER,
  started TEXT NOT NULL, last_seen TEXT, ended TEXT);
-- append-only history of every transition; no FK, so it outlives what it names
CREATE TABLE event(
  id INTEGER PRIMARY KEY,
  chunk INTEGER,
  at TEXT NOT NULL,
  by TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  text TEXT);
CREATE TABLE tr(key TEXT NOT NULL, lang TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY(key, lang));
CREATE INDEX chunk_plan ON chunk(plan, pos);
CREATE INDEX dep_chunk ON dep(chunk);
CREATE INDEX run_chunk ON run(chunk);
";

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub id: i64,
    pub app: String,
    pub name: String,
    pub title: String,
    pub area: String,
    pub state: String,
}

fn err(e: rusqlite::Error) -> String {
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
        conn.busy_timeout(Duration::from_secs(10)).map_err(err)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get::<_, String>(0))
            .map_err(err)?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(err)?;
        let mut s = Store { conn };
        s.migrate()?;
        Ok(s)
    }

    fn migrate(&mut self) -> Result<(), String> {
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
        if v == 0 {
            tx.execute_batch(SCHEMA).map_err(err)?;
            tx.pragma_update(None, "user_version", VERSION)
                .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    pub fn plans(&self) -> Result<Vec<PlanRow>, String> {
        let mut q = self
            .conn
            .prepare("SELECT id, app, name, title, area, state FROM plan ORDER BY app, name")
            .map_err(err)?;
        q.query_map([], |r| {
            Ok(PlanRow {
                id: r.get(0)?,
                app: r.get(1)?,
                name: r.get(2)?,
                title: r.get(3)?,
                area: r.get(4)?,
                state: r.get(5)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }

    /// Replace every plan of the apps in `plans` with what the markdown says, in one
    /// transaction. ponytail: a full replace — the markdown is the truth until b2's
    /// commands write the db; then import only adds plans the db does not hold.
    pub fn import_all(&mut self, plans: &[Import], now: &str) -> Result<Report, String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let mut apps: Vec<&str> = plans.iter().map(|p| p.app.as_str()).collect();
        apps.sort_unstable();
        apps.dedup();
        for app in &apps {
            tx.execute("DELETE FROM plan WHERE app = ?1", [app])
                .map_err(err)?;
        }
        let mut rep = Report::default();
        // (app, name) → (dir, [(chunk id, label, tick, raw)]) for the dep pass
        let mut ids: HashMap<(&str, &str), (&str, Chunks)> = HashMap::new();
        for p in plans {
            let chunks = insert_plan(&tx, p, now, &mut rep)?;
            ids.insert((&p.app, &p.md.name), (&p.dir, chunks));
        }
        for p in plans {
            let (_, own) = &ids[&(p.app.as_str(), p.md.name.as_str())];
            let mut prev: Option<(i64, Option<String>)> = None;
            for (id, label, tick, raw) in own {
                if *tick == Tick::Unknown {
                    continue; // fapony reads no unknown line: it is nobody's "chunk before"
                }
                if *tick == Tick::Open {
                    for (after, r) in deps(&p.app, raw, prev.as_ref(), own, &ids) {
                        if after.is_none() {
                            let who = format!(
                                "{}/{} {}",
                                p.app,
                                p.md.name,
                                label.as_deref().unwrap_or("?")
                            );
                            rep.unresolved.push(format!("{who} → {r}"));
                        }
                        tx.execute(
                            "INSERT INTO dep(chunk, after, ref) VALUES (?1, ?2, ?3)",
                            params![id, after, r],
                        )
                        .map_err(err)?;
                    }
                }
                prev = Some((*id, label.clone()));
            }
        }
        tx.commit().map_err(err)?;
        Ok(rep)
    }
}

type Chunks<'a> = Vec<(i64, Option<String>, Tick, &'a str)>;

fn insert_plan<'a>(
    tx: &rusqlite::Transaction,
    p: &'a Import,
    now: &str,
    rep: &mut Report,
) -> Result<Chunks<'a>, String> {
    let f = &p.md.front;
    let state = match p.dir.as_str() {
        "done" => "done",
        "parked" => "parked",
        _ if f.status.as_deref() == Some("blocked") => "blocked",
        _ => "active",
    };
    tx.execute(
        "INSERT INTO plan(app, name, title, area, kind, state, spec, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            p.app,
            p.md.name,
            p.md.title,
            f.area.as_deref().unwrap_or("dev"),
            f.kind,
            state,
            f.spec,
            p.source
        ],
    )
    .map_err(err)?;
    let plan = tx.last_insert_rowid();
    rep.plans += 1;
    let mut out = Vec::new();
    for (pos, it) in p.md.items.iter().enumerate() {
        let (state, wait) = match it.tick {
            Tick::Done => ("done", None),
            Tick::Dropped => ("dropped", None),
            Tick::Unknown => ("draft", None),
            Tick::Open if md::marker(&it.raw, "wip").is_some() => ("running", None),
            Tick::Open => match md::marker(&it.raw, "wait") {
                Some(w) => ("waiting", Some(w)),
                None => ("open", None),
            },
        };
        tx.execute(
            "INSERT INTO chunk(plan, pos, label, title, state, brief, wait_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?4, ?6)",
            params![plan, pos as i64, it.label, it.text, state, wait],
        )
        .map_err(err)?;
        let id = tx.last_insert_rowid();
        if state == "running" {
            tx.execute(
                "INSERT INTO run(chunk, branch, started) VALUES (?1, ?2, ?3)",
                params![id, md::marker(&it.raw, "wip"), now],
            )
            .map_err(err)?;
        }
        tx.execute(
            "INSERT INTO event(chunk, at, by, to_state, text) VALUES (?1, ?2, 'import', ?3, ?4)",
            params![id, now, state, p.source],
        )
        .map_err(err)?;
        rep.chunks += 1;
        rep.drafts += usize::from(it.tick == Tick::Unknown);
        out.push((id, it.label.clone(), it.tick, it.raw.as_str()));
    }
    Ok(out)
}

/// fapony `chunkLines`: the chunks labelled `want`, or — with none — the flat siblings it
/// was split into (2 → 2a, 2b). Unknown lines are no chunk.
fn chunk_lines(items: &Chunks, want: &str) -> Vec<i64> {
    let lbl = |l: &Option<String>| l.as_deref().map(str::to_lowercase);
    let known = items.iter().filter(|c| c.2 != Tick::Unknown);
    let exact: Vec<i64> = known
        .clone()
        .filter(|c| lbl(&c.1).as_deref() == Some(want))
        .map(|c| c.0)
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    known
        .filter(|c| {
            lbl(&c.1).is_some_and(|l| {
                let base = l
                    .strip_suffix(|ch: char| ch.is_ascii_lowercase())
                    .unwrap_or(&l);
                base == want
            })
        })
        .map(|c| c.0)
        .collect()
}

/// The `(after, ref)` rows of one open chunk: none marked = the chunk before it.
fn deps(
    app: &str,
    raw: &str,
    prev: Option<&(i64, Option<String>)>,
    own: &Chunks,
    ids: &HashMap<(&str, &str), (&str, Chunks)>,
) -> Vec<(Option<i64>, String)> {
    let Some(refs) = md::after_refs(raw) else {
        return prev
            .map(|(id, l)| (Some(*id), l.clone().unwrap_or_else(|| "previous".into())))
            .into_iter()
            .collect();
    };
    let mut out = Vec::new();
    for r in refs {
        let hits = match r.split_once(':') {
            // `vela-jobs:j4` — same `.fapony/`; a shipped plan (done/) meets it outright
            Some((plan, lbl)) => {
                match ids.get(&(app, plan.strip_prefix("plan-").unwrap_or(plan))) {
                    Some((dir, _)) if *dir == "done" => continue,
                    Some((dir, chunks)) if *dir == "plan" => chunk_lines(chunks, lbl),
                    _ => Vec::new(), // parked or missing never meets it
                }
            }
            None => chunk_lines(own, &r),
        };
        if hits.is_empty() {
            out.push((None, r));
        } else {
            out.extend(hits.into_iter().map(|h| (Some(h), r.clone())));
        }
    }
    out
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
