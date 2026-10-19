//! A plan's text as rows (plan fael-v8 `sections`): Goal, Scope, Done, Constraints and what
//! used to be its spec, one `section` row each, `key` stable per plan (`1`, `§3`, a heading
//! slug). Every write adds an `event` row of the plan (text `<key>: <title>`), so the plan's
//! rev moves and the sync feed can rebuild it; the newest one dates the section.

use super::store::err;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Deserialize;

/// What `fael plan new` writes: `(key, title, body)`.
pub(super) const SKELETON: [(&str, &str, &str); 4] = [
    ("1", "1. Goal (why)", ""),
    ("2", "2. Scope", "**Do:**\n**Don't:**"),
    ("3", "3. Done criteria", ""),
    ("4", "4. Constraints", ""),
];

/// A section whose body is empty or still its `SKELETON` body.
pub(super) fn blank(s: &Row) -> bool {
    let b = s.body.trim();
    b.is_empty() || SKELETON.iter().any(|(_, _, k)| *k == b)
}

/// One bill line: a new key adds a section (title needed), a known key edits it, `drop`
/// drops it. A field left out is left alone.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Part {
    pub key: String,
    title: Option<String>,
    body: Option<String>,
    #[serde(default)]
    drop: bool,
}

/// A section as export and the brief read it; `updated` = the local day of its newest event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub title: String,
    pub body: String,
    pub updated: String,
}

/// The key a heading gets: its leading number (`1. Goal` → `1`, `§3 Risks` → `§3`), else
/// its slug (`chunk rules` → `chunk-rules`).
pub(super) fn key_of(heading: &str) -> String {
    let h = heading.trim();
    let num: String = h
        .char_indices()
        .take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && c == '§'))
        .map(|(_, c)| c)
        .collect();
    if num.chars().any(|c| c.is_ascii_digit()) {
        return num;
    }
    let slug: String = h
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug
        .split('-')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>();
    if slug.is_empty() {
        "section".into()
    } else {
        slug.join("-")
    }
}

/// Markdown cut at each `## ` heading into `(key, title, body)`; text before the first one
/// is `intro`. A key met twice gets `-2`, `-3`, ….
pub(super) fn split(md: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    for l in md.lines() {
        match l.strip_prefix("## ") {
            Some(h) => {
                let base = key_of(h);
                let mut key = base.clone();
                for n in 2.. {
                    if !out.iter().any(|(k, ..)| *k == key) {
                        break;
                    }
                    key = format!("{base}-{n}");
                }
                out.push((key, h.trim().into(), String::new()));
            }
            None if out.is_empty() && l.trim().is_empty() => {}
            None => {
                if out.is_empty() {
                    out.push(("intro".into(), "Intro".into(), String::new()));
                }
                let body = &mut out.last_mut().expect("pushed").2;
                body.push_str(l);
                body.push('\n');
            }
        }
    }
    for s in &mut out {
        s.2 = s.2.trim().into();
    }
    out
}

/// The plan's event for a section write: `<key>: <title>` plus what happened.
fn event(tx: &Transaction, pid: i64, text: &str, now: &str) -> Result<(), String> {
    tx.execute(
        "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
         SELECT 'plan:' || app || '/' || name, ?1, 'owner', state, state, ?2, id
         FROM plan WHERE id = ?3",
        params![now, text, pid],
    )
    .map(drop)
    .map_err(err)
}

/// Add or edit section `key`; `None` = it already reads so.
pub(super) fn put(
    tx: &Transaction,
    pid: i64,
    key: &str,
    title: Option<&str>,
    body: Option<&str>,
    now: &str,
) -> Result<Option<String>, String> {
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return Err(format!(
            "rejected: section key {key:?} — one word, no spaces"
        ));
    }
    if title.is_some_and(|t| t.trim().is_empty()) {
        return Err(format!("rejected: section {key} needs a title"));
    }
    let cur: Option<(String, String)> = tx
        .query_row(
            "SELECT title, body FROM section WHERE plan = ?1 AND key = ?2",
            params![pid, key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(err)?;
    let (title, what) = match cur {
        None => {
            let t = title.ok_or(format!("rejected: section {key} is new — it needs a title"))?;
            tx.execute(
                "INSERT INTO section(plan, key, title, body, seq)
                 SELECT ?1, ?2, ?3, ?4, COALESCE(MAX(seq), 0) + 1024 FROM section WHERE plan = ?1",
                params![pid, key, t.trim(), body.unwrap_or_default()],
            )
            .map_err(err)?;
            (t.trim().to_string(), "added")
        }
        Some((t, b)) => {
            let title = title.map_or(t.clone(), |n| n.trim().to_string());
            let body = body.map_or(b.clone(), String::from);
            if title == t && body.trim() == b.trim() {
                return Ok(None);
            }
            tx.execute(
                "UPDATE section SET title = ?3, body = ?4 WHERE plan = ?1 AND key = ?2",
                params![pid, key, title, body],
            )
            .map_err(err)?;
            (title, "edited")
        }
    };
    event(tx, pid, &format!("{key}: {title}"), now)?;
    Ok(Some(format!("§ {key} {what}")))
}

/// A bill's section line, all or none with the rest of the bill.
pub(super) fn apply(
    tx: &Transaction,
    pid: i64,
    p: &Part,
    now: &str,
) -> Result<Option<String>, String> {
    if !p.drop {
        return put(tx, pid, &p.key, p.title.as_deref(), p.body.as_deref(), now);
    }
    let title: Option<String> = tx
        .query_row(
            "DELETE FROM section WHERE plan = ?1 AND key = ?2 RETURNING title",
            params![pid, p.key],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    let Some(title) = title else {
        return Ok(None);
    };
    event(tx, pid, &format!("{}: {title} — dropped", p.key), now)?;
    Ok(Some(format!("§ {} dropped", p.key)))
}

/// A plan's sections in order, each dated by its newest event — the machine's local day,
/// the day the agent writes in its own text (events are stamped UTC).
pub(super) fn list(c: &Connection, pid: i64) -> Result<Vec<Row>, String> {
    c.prepare(
        "SELECT s.key, s.title, s.body,
           COALESCE((SELECT date(MAX(e.at), 'localtime') FROM event e
                     WHERE e.plan = s.plan AND e.chunk_uid LIKE 'plan:%'
                       AND substr(e.text, 1, length(s.key) + 2) = s.key || ': '), '')
         FROM section s WHERE s.plan = ?1 ORDER BY s.seq",
    )
    .map_err(err)?
    .query_map([pid], |r| {
        Ok(Row {
            key: r.get(0)?,
            title: r.get(1)?,
            body: r.get(2)?,
            updated: r.get(3)?,
        })
    })
    .map_err(err)?
    .collect::<Result<_, _>>()
    .map_err(err)
}

/// `--reads`: each `<key>` of the chunk's plan or `<plan>/<key>`, one word; one per line.
/// A key not there yet is fine — the brief says it is not found.
pub(super) fn reads(refs: &[String]) -> Result<Option<String>, String> {
    if let Some(r) = refs
        .iter()
        .find(|r| r.ends_with('/') || r.chars().any(char::is_whitespace))
    {
        return Err(format!(
            "rejected: reads {r:?} — a section key, or <plan>/<key> for another plan's"
        ));
    }
    Ok((!refs.is_empty()).then(|| refs.join("\n")))
}

/// What a section ref names: the exact key first (like chunk lookup's uid),
/// then one normalized hit — key, title or the title without its leading number
/// (`1. Goal (why)` → `Goal (why)`), `-`/`_`/` ` folded, case-insensitive. Zero or
/// several hits = no hit: the caller lists the plan's keys as before, so a guess
/// never lands on the wrong section.
pub(super) fn resolve<'a>(secs: &'a [Row], want: &str) -> Option<&'a Row> {
    if let Some(r) = secs.iter().find(|s| s.key == want) {
        return Some(r);
    }
    let norm = |v: &str| {
        v.to_lowercase()
            .chars()
            .map(|c| if c == '-' || c == '_' { ' ' } else { c })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    fn strip(t: &str) -> &str {
        t.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '§' || c == ' ')
    }
    let w = norm(want);
    let keys: Vec<&Row> = secs.iter().filter(|s| norm(&s.key) == w).collect();
    if keys.len() == 1 {
        return Some(keys[0]);
    }
    if keys.len() > 1 {
        return None;
    }
    let titles: Vec<&Row> = secs
        .iter()
        .filter(|s| norm(&s.title) == w || norm(strip(&s.title)) == w)
        .collect();
    if titles.len() == 1 {
        return Some(titles[0]);
    }
    None
}

/// The section a read names, from plan `pid` or the plan before its last `/` (`None` = no
/// such plan or key). Another plan's keeps the whole ref as its key.
pub(super) fn read(c: &Connection, pid: i64, r: &str) -> Result<Option<Row>, String> {
    let (other, key) = match r.rsplit_once('/') {
        Some((plan, key)) => {
            let id: Option<i64> = c
                .query_row(
                    "SELECT id FROM plan WHERE name = ?1 OR (app <> '' AND app || '/' || name = ?1)
                     ORDER BY app = '' DESC LIMIT 1",
                    [plan],
                    |q| q.get(0),
                )
                .optional()
                .map_err(err)?;
            let Some(id) = id else { return Ok(None) };
            (id, key)
        }
        None => (pid, r),
    };
    let secs = list(c, other)?;
    let row = resolve(&secs, key).cloned();
    Ok(row.map(|s| Row {
        key: if other == pid { s.key } else { r.into() },
        ..s
    }))
}

/// Once, on a db from before the `section` table (epoch v8's step, before it drops
/// `plan.doc`): every plan's `plan.doc` cut into sections.
pub(super) fn carry(tx: &Transaction) -> Result<(), String> {
    let docs: Vec<(i64, String)> = tx
        .prepare("SELECT id, doc FROM plan WHERE COALESCE(doc, '') <> ''")
        .map_err(err)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let now = crate::rfc3339(crate::now_ms());
    for (pid, doc) in docs {
        for (key, title, body) in split(&doc) {
            put(tx, pid, &key, Some(&title), Some(&body), &now)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "section_tests.rs"]
mod tests;
