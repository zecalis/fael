//! `fael plan export`: one plan as markdown, for review and backup — the db is a binary
//! git cannot merge (SPEC §7). Checkbox lines stay in the import grammar, so a snapshot
//! reads back with `fael plan import`; each chunk's db fields follow on an indented line.
//! The plan's own text (Goal, Scope, Done, Constraints, …) follows the chunks: its sections
//! in order, each dated by its newest event — else, for a plan not migrated yet, the md's.

use super::cutover::BANNER;
use super::md;
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

/// `md` reads a repo-relative file (the plan's `source`), `None` when it is gone.
pub fn export(s: &Store, plan: i64, md: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let [
        app,
        name,
        title,
        area,
        kind,
        state,
        spec,
        source,
        truth,
        refs,
    ] = s
        .conn
        .query_row(
            "SELECT app, name, title, area, kind, state, spec, source, truth, refs
             FROM plan WHERE id = ?1",
            [plan],
            cols,
        )
        .map_err(err)?;
    let mut o = String::from("---\n");
    for (k, v) in [("kind", &kind), ("area", &area), ("spec", &spec)] {
        if !v.is_empty() {
            let _ = writeln!(o, "{k}: {v}");
        }
    }
    if state == "blocked" {
        o.push_str("status: blocked\n");
    }
    let key = if app.is_empty() {
        name
    } else {
        format!("{app}/{name}")
    };
    let _ = writeln!(
        o,
        "---\n\n# {title}\n\n> fael plan export — {key} · truth {truth} · {state} · source {source}"
    );
    for r in refs.lines().filter(|r| !r.is_empty()) {
        let _ = writeln!(o, "> ref: {r}");
    }
    o.push_str("\n## TL;DR\n");
    chunks(s, plan, &mut o)?;
    let secs = section::list(&s.conn, plan)?;
    if secs.is_empty() {
        let text = (!source.is_empty())
            .then(|| md(&source))
            .flatten()
            .map(|t| prose(&t))
            .unwrap_or_default();
        if !text.trim().is_empty() {
            let _ = writeln!(o, "\n{}", text.trim());
        }
    }
    for r in &secs {
        o.push('\n');
        o.push_str(&one(r));
    }
    Ok(o)
}

/// A section under its heading, its key and the day it last changed below it.
fn one(r: &Row) -> String {
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
    secs.iter().find(|r| r.key == key).map(one).ok_or_else(|| {
        let keys: Vec<&str> = secs.iter().map(|r| r.key.as_str()).collect();
        format!(
            "rejected: no section {key} — the plan has: {}",
            keys.join(", ")
        )
    })
}

/// An md plan's text minus what the export already wrote: front matter, title, and the
/// TL;DR's heading and chunk lines (checkboxes, the cutover banner) — its prose stays.
fn prose(text: &str) -> String {
    let mut out = String::new();
    let mut h2 = 0;
    for l in text.lines() {
        h2 += usize::from(md::is_h2(l));
        let tldr = h2 == 1 && l.trim_start_matches(['#', ' ']).starts_with("TL;DR");
        let chunk = h2 == 1 && (md::checkbox(l).is_some() || l.trim_start().starts_with(BANNER));
        if h2 > 0 && !tldr && !chunk {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

fn chunks(s: &Store, plan: i64, o: &mut String) -> Result<(), String> {
    let mut q = s
        .conn
        .prepare(
            "SELECT c.title, c.brief, c.state, c.uid,
               (SELECT group_concat(COALESCE(a.label, a.uid, e.ref), ', ')
                  FROM edge e LEFT JOIN chunk a ON a.id = e.dst
                 WHERE e.src = c.id AND e.kind = 'after'),
               (SELECT r.start || ' ' || COALESCE(r.branch, '') FROM run r
                 WHERE r.chunk = c.id AND r.ended IS NULL ORDER BY r.id DESC LIMIT 1),
               c.wait_on, c.wait_text, c.wait_until, c.size, c.client_hint, c.model_hint, c.scope,
               c.due, c.pin, c.approved_model
             FROM chunk c WHERE c.plan = ?1 ORDER BY c.seq",
        )
        .map_err(err)?;
    let names = [
        "after", "run", "wait on", "wait", "until", "size", "client", "model", "scope", "due",
        "pin", "approved",
    ];
    for row in q.query_map([plan], cols::<16>).map_err(err)? {
        let [title, brief, state, uid, rest @ ..] = row.map_err(err)?;
        let tick = match state.as_str() {
            "done" => 'x',
            "dropped" | "replaced" => '~',
            "draft" => '?',
            _ => ' ',
        };
        // a draft from an unknown checkbox keeps its whole line as the title
        if tick == '?' && title.starts_with(['-', '*']) {
            o.push_str(&title);
        } else {
            let _ = write!(o, "- [{tick}] {title}");
        }
        let _ = write!(o, "\n  uid {uid} · {state}");
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
    }
    Ok(())
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
    let tx = s.conn.unchecked_transaction().map_err(err)?;
    let [app, name, title, area, state, refs, repo] = tx
        .query_row(
            "SELECT app, name, title, area, state, COALESCE(refs, ''), COALESCE(repo, '')
             FROM plan WHERE id = ?1",
            [plan],
            cols,
        )
        .map_err(err)?;
    let key = if app.is_empty() {
        name
    } else {
        format!("{app}/{name}")
    };
    let mut q = tx
        .prepare(
            "SELECT c.uid, c.label, c.title, c.brief, c.size, c.model_hint, c.client_hint,
                    c.scope,
                    (SELECT group_concat(a.uid, char(10)) FROM edge e JOIN chunk a ON a.id = e.dst
                      WHERE e.src = c.id AND e.kind = 'after')
             FROM chunk c WHERE c.plan = ?1 AND c.state NOT IN ('replaced', 'dropped')
             ORDER BY c.seq",
        )
        .map_err(err)?;
    let lines = |t: Option<String>| -> Vec<String> {
        t.iter().flat_map(|t| t.lines()).map(String::from).collect()
    };
    let chunks: Vec<Json> = q
        .query_map(params![plan], |r| {
            let g = |i| r.get::<_, Option<String>>(i);
            Ok(json!({
                "uid": g(0)?, "label": g(1)?, "title": g(2)?, "brief": g(3)?, "size": g(4)?,
                "model": g(5)?, "client": g(6)?, "scope": lines(g(7)?), "after": lines(g(8)?),
            }))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let sections: Vec<Json> = section::list(&tx, plan)?
        .into_iter()
        .map(|r| json!({"key": r.key, "title": r.title, "body": r.body}))
        .collect();
    Ok(json!({
        "v": 1, "plan": key, "rev": rev(&tx, plan)?, "title": title, "area": area,
        "state": state, "refs": lines(Some(refs)), "repo": repo, "sections": sections,
        "chunks": chunks,
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
