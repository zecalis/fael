//! `fael plan export`: one plan as markdown, for review and backup — the db is a binary
//! git cannot merge (SPEC §7). One checkbox line per chunk, its db fields on an indented
//! line. The plan's own text (Goal, Scope, Done, Constraints, …) follows the chunks: its
//! sections in order, each dated by its newest event.

use super::section::{self, Row};
use super::store::{Store, err};
use rusqlite::types::Value;
use rusqlite::{Connection, params};
use serde_json::{Value as Json, json};
use std::fmt::Write;

/// The first `N` columns as text (NULL = empty), whatever their SQLite type.
fn cols<const N: usize>(r: &rusqlite::Row) -> rusqlite::Result<[String; N]> {
    let mut out = std::array::from_fn(|_| String::new());
    for (i, o) in out.iter_mut().enumerate() {
        *o = match r.get::<_, Value>(i)? {
            Value::Integer(n) => n.to_string(),
            Value::Text(t) => t,
            _ => String::new(),
        };
    }
    Ok(out)
}

pub fn export(s: &Store, plan: i64) -> Result<String, String> {
    let [app, name, title, area, state] = s
        .conn
        .query_row(
            "SELECT app, name, title, area, state FROM plan WHERE id = ?1",
            [plan],
            cols,
        )
        .map_err(err)?;
    let mut o = format!("---\narea: {area}\n");
    if state == "blocked" {
        o.push_str("status: blocked\n");
    }
    let key = if app.is_empty() {
        name
    } else {
        format!("{app}/{name}")
    };
    let _ = write!(
        o,
        "---\n\n# {title}\n\n> fael plan export — {key} · {state}"
    );
    if let Some(p) = parent_of(&s.conn, plan)? {
        let _ = write!(o, " · parent {p}");
    }
    o.push('\n');
    o.push_str("\n## TL;DR\n");
    chunks(s, plan, &mut o)?;
    for r in &section::list(&s.conn, plan)? {
        o.push('\n');
        o.push_str(&one(r));
    }
    Ok(o)
}

/// A section under its heading, its key and the day it last changed below it.
pub(super) fn one(r: &Row) -> String {
    let day = r.updated.get(..10).unwrap_or(&r.updated);
    let mut o = format!("## {}\n> section {} · updated {day}\n", r.title, r.key);
    if !r.body.trim().is_empty() {
        let _ = writeln!(o, "\n{}", r.body.trim());
    }
    o
}

/// `fael plan export <plan> --section <key>`: that section alone.
pub fn export_section(s: &Store, plan: i64, key: &str) -> Result<String, String> {
    let secs = section::list(&s.conn, plan)?;
    section::resolve(&secs, key).map(one).ok_or_else(|| {
        let keys: Vec<&str> = secs.iter().map(|r| r.key.as_str()).collect();
        format!(
            "rejected: no section {key} — the plan has: {}",
            keys.join(", ")
        )
    })
}

/// Uids of `plan` in plan order: after edges first, seq breaks ties. A cycle
/// (add_after refuses them) falls back to seq for the rest.
fn topo_order(c: &Connection, plan: i64) -> Result<Vec<String>, String> {
    let mut ids: Vec<(i64, i64, String)> = c
        .prepare("SELECT id, seq, uid FROM chunk WHERE plan = ?1 ORDER BY seq")
        .map_err(err)?
        .query_map([plan], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    ids.sort_by_key(|(_, seq, _)| *seq);
    let pos: std::collections::HashMap<i64, usize> = ids
        .iter()
        .enumerate()
        .map(|(i, (id, _, _))| (*id, i))
        .collect();
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    let mut rdeps: Vec<Vec<usize>> = vec![Vec::new(); ids.len()];
    let mut e = c
        .prepare(
            "SELECT e.src, e.dst FROM edge e JOIN chunk s ON s.id = e.src
              JOIN chunk d ON d.id = e.dst
             WHERE s.plan = ?1 AND d.plan = ?1 AND e.kind = 'after'",
        )
        .map_err(err)?;
    for r in e
        .query_map([plan], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(err)?
    {
        let (src, dst): (i64, i64) = r.map_err(err)?;
        if let (Some(&si), Some(&di)) = (pos.get(&src), pos.get(&dst))
            && !deps[si].contains(&di)
        {
            deps[si].push(di);
            rdeps[di].push(si);
        }
    }
    let mut indeg: Vec<usize> = deps.iter().map(Vec::len).collect();
    let mut ready: Vec<usize> = (0..ids.len()).filter(|&i| indeg[i] == 0).collect();
    ready.sort_by_key(|&i| ids[i].1);
    let mut out: Vec<String> = Vec::with_capacity(ids.len());
    let mut done = vec![false; ids.len()];
    while !ready.is_empty() || out.len() < ids.len() {
        let next = if let Some(&i) = ready.first() {
            ready.remove(0);
            i
        } else {
            (0..ids.len())
                .filter(|&i| !done[i])
                .min_by_key(|&i| ids[i].1)
                .unwrap()
        };
        if done[next] {
            continue;
        }
        done[next] = true;
        out.push(ids[next].2.clone());
        for &dep in &rdeps[next].clone() {
            indeg[dep] = indeg[dep].saturating_sub(1);
            if indeg[dep] == 0 && !done[dep] && !ready.contains(&dep) {
                ready.push(dep);
            }
        }
        ready.sort_by_key(|&i| ids[i].1);
    }
    Ok(out)
}

fn chunks(s: &Store, plan: i64, o: &mut String) -> Result<(), String> {
    let order = topo_order(&s.conn, plan)?;
    let mut q = s
        .conn
        .prepare(
            "SELECT c.title, c.brief, c.state, c.uid, c.label,
               (SELECT group_concat(COALESCE(a.label, a.uid, e.ref), ', ')
                  FROM edge e LEFT JOIN chunk a ON a.id = e.dst
                 WHERE e.src = c.id AND e.kind = 'after'),
               (SELECT r.start || ' ' || COALESCE(r.branch, '') FROM run r
                 WHERE r.chunk = c.id AND r.ended IS NULL ORDER BY r.id DESC LIMIT 1),
               c.wait_on, c.wait_text, c.wait_until, c.size, c.model_hint, c.scope,
               c.approved_model, c.reads
             FROM chunk c WHERE c.plan = ?1 ORDER BY c.seq",
        )
        .map_err(err)?;
    let names = [
        "after", "run", "wait on", "wait", "until", "size", "model", "scope", "approved", "reads",
    ];
    // the notes the chunk's next brief prints: those since its last start
    let mut notes = s
        .conn
        .prepare(
            "SELECT substr(at, 1, 10), text FROM event
             WHERE chunk_uid = ?1 AND by = 'note' AND text IS NOT NULL
               AND id > COALESCE((SELECT MAX(id) FROM event WHERE chunk_uid = ?1
                                    AND to_state = 'running' AND by != 'note'), 0)
             ORDER BY id",
        )
        .map_err(err)?;
    // the owner's close reason on a dropped or parked chunk: without it the
    // reason lives only in the event feed, and the markdown reads bare `dropped`
    let mut closed = s
        .conn
        .prepare(
            "SELECT substr(at, 1, 10), text FROM event
              WHERE chunk_uid = ?1 AND by = 'owner'
                AND to_state IN ('dropped', 'parked')
                AND text IS NOT NULL AND text != ''
              ORDER BY id DESC LIMIT 1",
        )
        .map_err(err)?;
    let mut rows: Vec<[String; 15]> = q
        .query_map([plan], cols::<15>)
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let at: std::collections::HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, u)| (u.as_str(), i))
        .collect();
    rows.sort_by_key(|r| at.get(r[3].as_str()).copied().unwrap_or(usize::MAX));
    for [title, brief, state, uid, label, rest @ ..] in &rows {
        let tick = match state.as_str() {
            "done" => 'x',
            "dropped" | "replaced" => '~',
            "draft" => '?',
            _ => ' ',
        };
        // a draft from an unknown checkbox keeps its whole line as the title
        if tick == '?' && title.starts_with(['-', '*']) {
            o.push_str(title);
        } else {
            let _ = write!(o, "- [{tick}] {title}");
        }
        let _ = write!(o, "\n  uid {uid}");
        if !label.is_empty() {
            let _ = write!(o, " · label {label}");
        }
        let _ = write!(o, " · {state}");
        for (k, v) in names.iter().zip(rest) {
            if !v.trim().is_empty() {
                let _ = write!(o, " · {k} {}", v.trim().replace('\n', ", "));
            }
        }
        o.push('\n');
        if brief != title {
            for l in brief.lines() {
                let _ = writeln!(o, "  > {l}");
            }
        }
        if matches!(state.as_str(), "dropped" | "parked") {
            let mut q = closed.query_map([uid.as_str()], cols::<2>).map_err(err)?;
            if let Some(n) = q.next() {
                let [day, text] = n.map_err(err)?;
                let _ = writeln!(o, "  > {state} {day}: {}", text.replace('\n', " "));
            }
        }
        for n in notes.query_map([uid.as_str()], cols::<2>).map_err(err)? {
            let [day, text] = n.map_err(err)?;
            let _ = writeln!(o, "  > note {day}: {}", text.replace('\n', " "));
        }
    }
    Ok(())
}

/// The umbrella plan's name; `None` = top level.
fn parent_of(c: &Connection, plan: i64) -> Result<Option<String>, String> {
    c.query_row("SELECT parent FROM plan WHERE id = ?1", [plan], |r| {
        r.get(0)
    })
    .map_err(err)
}

/// A plan's rev: the newest `event` row that belongs to it (0 = none yet). Every write to
/// the plan or one of its chunks adds one, so a bill read at rev N is stale once it moves.
pub(super) fn rev(c: &Connection, plan: i64) -> Result<i64, String> {
    c.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM event WHERE plan = ?1",
        [plan],
        |r| r.get(0),
    )
    .map_err(err)
}

/// `fael plan export <plan> --json`: the bill `fael plan apply` takes back (decision
/// fael-board:plan-bill, `"v": 1`) — the header and a line per chunk still planned (not
/// `replaced` or `dropped`), read in one transaction so `rev` matches the lines.
pub fn export_json(s: &Store, plan: i64) -> Result<Json, String> {
    let order = topo_order(&s.conn, plan)?;
    let tx = s.conn.unchecked_transaction().map_err(err)?;
    let [app, name, title, area, state] = tx
        .query_row(
            "SELECT app, name, title, area, state FROM plan WHERE id = ?1",
            [plan],
            cols,
        )
        .map_err(err)?;
    let parent = parent_of(&tx, plan)?;
    let key = if app.is_empty() {
        name
    } else {
        format!("{app}/{name}")
    };
    let mut q = tx
        .prepare(
            "SELECT c.uid, c.label, c.title, c.brief, c.size, c.model_hint, c.scope, c.reads,
                    (SELECT group_concat(a.uid, char(10)) FROM edge e JOIN chunk a ON a.id = e.dst
                      WHERE e.src = c.id AND e.kind = 'after')
             FROM chunk c WHERE c.plan = ?1 AND c.state NOT IN ('replaced', 'dropped')
             ORDER BY c.seq",
        )
        .map_err(err)?;
    let lines = |t: Option<String>| -> Vec<String> {
        t.iter().flat_map(|t| t.lines()).map(String::from).collect()
    };
    let mut chunks: Vec<Json> = q
        .query_map(params![plan], |r| {
            let g = |i| r.get::<_, Option<String>>(i);
            Ok(json!({
                "uid": g(0)?, "label": g(1)?, "title": g(2)?, "brief": g(3)?, "size": g(4)?,
                "model": g(5)?, "scope": lines(g(6)?), "reads": lines(g(7)?),
                "after": lines(g(8)?),
            }))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let at: std::collections::HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, u)| (u.as_str(), i))
        .collect();
    chunks.sort_by_key(|c| {
        c.get("uid")
            .and_then(|u| u.as_str())
            .and_then(|u| at.get(u).copied())
            .unwrap_or(usize::MAX)
    });
    let sections: Vec<Json> = section::list(&tx, plan)?
        .into_iter()
        .map(|r| json!({"key": r.key, "title": r.title, "body": r.body}))
        .collect();
    Ok(json!({
        "v": 1, "plan": key, "rev": rev(&tx, plan)?, "title": title, "area": area,
        "parent": parent, "state": state, "sections": sections, "chunks": chunks,
    }))
}

/// Every column of each row `sql` returns, as a JSON object keyed on its name — whole rows,
/// so a column added later rides along without touching the feed.
fn rows(c: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Json>, String> {
    let mut q = c.prepare(sql).map_err(err)?;
    let names: Vec<String> = q.column_names().into_iter().map(String::from).collect();
    q.query_map(p, |r| {
        let mut o = serde_json::Map::new();
        for (i, n) in names.iter().enumerate() {
            let v = match r.get::<_, Value>(i)? {
                Value::Integer(n) => json!(n),
                Value::Real(f) => json!(f),
                Value::Text(t) => json!(t),
                Value::Null | Value::Blob(_) => Json::Null,
            };
            o.insert(n.clone(), v);
        }
        Ok(Json::Object(o))
    })
    .map_err(err)?
    .collect::<Result<_, _>>()
    .map_err(err)
}

/// `fael plan export --since <event-id>`: the change feed fael-sync and any backup read
/// (decision fael-board:sync-backup, `"v": 1`) — the events after `since` and the current
/// rows they touched, whole, since an event alone can't rebuild a brief edit. `since` 0 =
/// everything. `head` is the next call's `since`. One read transaction, no side effect.
// ponytail: chunk_issue, contract and tr stay out until a reader needs them
pub fn feed(s: &Store, since: i64) -> Result<Json, String> {
    let tx = s.conn.unchecked_transaction().map_err(err)?;
    let db_id: String = tx
        .query_row("SELECT value FROM meta WHERE key = 'db_id'", [], |r| {
            r.get(0)
        })
        .map_err(err)?;
    let head: i64 = tx
        .query_row("SELECT COALESCE(MAX(id), 0) FROM event", [], |r| r.get(0))
        .map_err(err)?;
    let touched = "SELECT chunk_uid FROM event WHERE id > ?1";
    let chunks = format!("SELECT id FROM chunk WHERE uid IN ({touched})");
    Ok(json!({
        "v": 1, "db_id": db_id, "head": head,
        "events": rows(&tx, "SELECT * FROM event WHERE id > ?1 ORDER BY id", [since])?,
        "plans": rows(&tx, "SELECT * FROM plan WHERE id IN
                   (SELECT plan FROM event WHERE id > ?1) ORDER BY id", [since])?,
        // every section of a touched plan: one missing from the set was dropped
        "sections": rows(&tx, "SELECT * FROM section WHERE plan IN
                   (SELECT plan FROM event WHERE id > ?1) ORDER BY plan, seq", [since])?,
        "chunks": rows(&tx, &format!("SELECT * FROM chunk WHERE id IN ({chunks}) ORDER BY id"), [since])?,
        "edges": rows(&tx, &format!(
            "SELECT s.uid AS src, d.uid AS dst, e.ref, e.kind FROM edge e
             JOIN chunk s ON s.id = e.src LEFT JOIN chunk d ON d.id = e.dst
             WHERE e.src IN ({chunks}) ORDER BY s.id"), [since])?,
        "runs": rows(&tx, &format!("SELECT * FROM run WHERE chunk IN ({chunks}) ORDER BY id"), [since])?,
    }))
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
