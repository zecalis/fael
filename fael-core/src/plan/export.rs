//! `fael plan export`: one plan as markdown, for review and backup — the db is a binary
//! git cannot merge (SPEC §7). Checkbox lines stay in the import grammar, so a snapshot
//! reads back with `fael plan import`; each chunk's db fields follow on an indented line.

use super::store::{Store, err};
use rusqlite::types::Value;
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
    Ok(o)
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
               c.wait_on, c.wait_text, c.wait_until, c.size, c.model_hint, c.scope, c.due,
               c.pin, c.approved_model
             FROM chunk c WHERE c.plan = ?1 ORDER BY c.seq",
        )
        .map_err(err)?;
    let names = [
        "after", "run", "wait on", "wait", "until", "size", "model", "scope", "due", "pin",
        "approved",
    ];
    for row in q.query_map([plan], cols::<15>).map_err(err)? {
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
                let _ = write!(o, " · {k} {}", v.trim());
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
