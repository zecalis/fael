//! `fael plan apply` (decision fael-board:plan-bill): a whole plan edited at once, like a
//! bill — header plus lines — in one `BEGIN IMMEDIATE` on top of the `chunk apply` engine.
//! The `rev` in the bill must still be the plan's rev (`MAX(event.id)` of the plan), else
//! nothing is written. A line missing from the bill is left alone; a running or finished
//! chunk rejects the bill if any line changes it. Every row it writes shares one batch.

use super::State;
use super::apply::{Op, insert, run};
use super::chunk::{Fields, add_after, batch, edit_in, last_event, row, set, tx, uid_id};
use super::export::rev;
use super::section::{self, Part};
use super::store::{Store, err};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Deserialize;

/// The bill, as `fael plan export <plan> --json` writes it. A key left out is left alone;
/// `""` clears a text field.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bill {
    v: u32,
    /// `name` or `app/name`
    plan: String,
    rev: i64,
    title: Option<String>,
    area: Option<String>,
    /// read back, never set: `fael plan park|unpark|done` moves it
    state: Option<String>,
    /// by key: one left out is left alone
    #[serde(default)]
    sections: Vec<Part>,
    #[serde(default)]
    chunks: Vec<Line>,
    #[serde(default)]
    ops: Vec<Op>,
}

/// One line: no `uid` adds a chunk, a `uid` edits it, `drop` drops it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    uid: Option<String>,
    #[serde(default)]
    drop: bool,
    label: Option<String>,
    title: Option<String>,
    brief: Option<String>,
    size: Option<String>,
    model: Option<String>,
    scope: Option<Vec<String>>,
    reads: Option<Vec<String>>,
    /// the full set of chunks it waits on
    after: Option<Vec<String>>,
}

/// `new` when it differs from `old` (the bill sends back what it read).
fn changed(new: &Option<String>, old: &str) -> Option<String> {
    new.as_ref().filter(|n| n.trim() != old.trim()).cloned()
}

fn header(tx: &Transaction, pid: i64, b: &Bill, now: &str) -> Result<Option<String>, String> {
    let (app, name, state, cur): (String, String, String, [String; 2]) = tx
        .query_row(
            "SELECT app, name, state, title, area FROM plan WHERE id = ?1",
            [pid],
            |r| {
                let g = |i| r.get::<_, String>(i);
                Ok((g(0)?, g(1)?, g(2)?, [g(3)?, g(4)?]))
            },
        )
        .map_err(err)?;
    if b.state.as_deref().is_some_and(|s| s != state) {
        return Err(format!(
            "rejected: plan {name} is {state} — `fael plan park|unpark|done` moves it, a bill does not"
        ));
    }
    let mut what = Vec::new();
    for (col, new, old) in [("title", &b.title, &cur[0]), ("area", &b.area, &cur[1])] {
        let Some(v) = new.as_ref().filter(|n| n != &old) else {
            continue;
        };
        if col == "title" && v.trim().is_empty() {
            return Err("rejected: a plan needs a title".into());
        }
        if col == "area" && !matches!(v.as_str(), "dev" | "marketing") {
            return Err(format!("rejected: area {v} — dev or marketing"));
        }
        tx.execute(
            &format!("UPDATE plan SET {col} = NULLIF(?1, '') WHERE id = ?2"),
            params![v, pid],
        )
        .map_err(err)?;
        what.push(col);
    }
    if what.is_empty() {
        return Ok(None);
    }
    let text = format!("edited {}", what.join(", "));
    tx.execute(
        "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
         VALUES (?1, ?2, 'owner', ?3, ?3, ?4, ?5)",
        params![format!("plan:{app}/{name}"), now, state, text, pid],
    )
    .map_err(err)?;
    Ok(Some(format!("plan {name}: {text}")))
}

/// The `after` edges of chunk `id` that name a chunk.
fn waits(tx: &Transaction, id: i64) -> Result<Vec<i64>, String> {
    tx.prepare("SELECT dst FROM edge WHERE src = ?1 AND kind = 'after' AND dst IS NOT NULL")
        .map_err(err)?
        .query_map([id], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}

fn add_line(tx: &Transaction, pid: i64, l: &Line, now: &str) -> Result<String, String> {
    if l.drop {
        return Err("rejected: drop:true needs the uid of the chunk to drop".into());
    }
    let f = Fields {
        title: l.title.clone(),
        brief: l.brief.clone(),
        size: l.size.clone(),
        model: l.model.clone(),
        scope: l.scope.clone(),
        reads: l.reads.clone(),
        label: l.label.clone(),
        ..Fields::default()
    };
    let seq = tx
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1024 FROM chunk WHERE plan = ?1",
            [pid],
            |r| r.get(0),
        )
        .map_err(err)?;
    let (id, uid) = insert(tx, pid, seq, &f, "added by plan apply", now)?;
    for a in l.after.iter().flatten() {
        add_after(tx, id, uid_id(tx, pid, a)?)?;
    }
    let state = if l.brief.as_deref().is_some_and(|b| !b.trim().is_empty()) {
        "open"
    } else {
        "draft"
    };
    Ok(format!(
        "+ {uid} {} ({state})",
        l.title.as_deref().unwrap_or_default().trim()
    ))
}

fn edit_line(
    tx: &Transaction,
    pid: i64,
    uid: &str,
    l: &Line,
    now: &str,
) -> Result<Option<String>, String> {
    let r = row(tx, uid)?;
    let (plan, cur): (i64, [String; 7]) = tx
        .query_row(
            "SELECT plan, title, brief, COALESCE(size, ''), COALESCE(model_hint, ''),
                    COALESCE(scope, ''), COALESCE(label, ''), COALESCE(reads, '')
             FROM chunk WHERE id = ?1",
            [r.id],
            |q| {
                let g = |i| q.get::<_, String>(i);
                Ok((q.get(0)?, [g(1)?, g(2)?, g(3)?, g(4)?, g(5)?, g(6)?, g(7)?]))
            },
        )
        .map_err(err)?;
    if plan != pid {
        return Err(format!("rejected: chunk {uid} is in another plan"));
    }
    let f = Fields {
        title: changed(&l.title, &cur[0]),
        brief: changed(&l.brief, &cur[1]),
        size: changed(&l.size, &cur[2]),
        model: changed(&l.model, &cur[3]),
        scope: l.scope.clone().filter(|s| s.join("\n") != cur[4]),
        reads: l.reads.clone().filter(|s| s.join("\n") != cur[6]),
        label: changed(&l.label, &cur[5]),
        ..Fields::default()
    };
    let edits = f.title.is_some()
        || f.brief.is_some()
        || f.size.is_some()
        || f.model.is_some()
        || f.scope.is_some()
        || f.reads.is_some()
        || f.label.is_some();
    let have = waits(tx, r.id)?;
    let want = match &l.after {
        Some(a) => a
            .iter()
            .map(|u| uid_id(tx, pid, u))
            .collect::<Result<Vec<_>, _>>()?,
        None => have.clone(),
    };
    let adds: Vec<i64> = want.iter().filter(|w| !have.contains(w)).copied().collect();
    let cuts: Vec<i64> = have.iter().filter(|h| !want.contains(h)).copied().collect();
    let changes = l.drop || edits || !adds.is_empty() || !cuts.is_empty();
    let dropped_again = l.drop && r.state == State::Dropped;
    if changes && (r.state == State::Running || (r.state.terminal() && !dropped_again)) {
        return Err(format!(
            "rejected: chunk {uid} is {} — a bill changes nothing that runs or is finished",
            r.state.as_str()
        ));
    }
    if l.drop {
        if r.state == State::Dropped {
            return Ok(None);
        }
        set(
            tx,
            &r,
            State::Dropped,
            "owner",
            Some("dropped by plan apply"),
            now,
        )?;
        super::chunk::end_live(tx, r.id, now)?;
        return Ok(Some(format!("- {uid} dropped")));
    }
    let mut out = Vec::new();
    if edits {
        out.push(edit_in(tx, uid, &f, now)?);
    }
    if !adds.is_empty() || !cuts.is_empty() {
        for &d in &adds {
            add_after(tx, r.id, d)?;
        }
        for &d in &cuts {
            tx.execute(
                "DELETE FROM edge WHERE src = ?1 AND dst = ?2 AND kind = 'after'",
                params![r.id, d],
            )
            .map_err(err)?;
        }
        let text = format!("after: +{} −{}", adds.len(), cuts.len());
        let r = row(tx, uid)?;
        set(tx, &r, r.state, "owner", Some(&text), now)?;
        out.push(text);
    }
    Ok((!out.is_empty()).then(|| format!("~ {uid} {}", out.join(" · "))))
}

impl Store {
    /// `fael plan apply`: header, then each line, then the ops, all or none. With `dry`
    /// the transaction rolls back after it has said what it would write. Returns the
    /// lines and the plan's rev now.
    pub fn bill(&mut self, b: Bill, now: &str, dry: bool) -> Result<(Vec<String>, i64), String> {
        if b.v != 1 {
            return Err(format!("rejected: bill v {} — this fael reads v 1", b.v));
        }
        let tx = tx(self)?;
        let pid: i64 = tx
            .query_row(
                "SELECT id FROM plan
                 WHERE name = ?1 OR (app <> '' AND app || '/' || name = ?1)
                 ORDER BY app = '' DESC LIMIT 1",
                [&b.plan],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?
            .ok_or(format!("rejected: no plan '{}' in the bill", b.plan))?;
        let now_rev = rev(&tx, pid)?;
        if now_rev != b.rev {
            return Err(format!(
                "rejected: stale rev — the bill has {}, the plan is at {now_rev}; `fael plan export {} --json` again",
                b.rev, b.plan
            ));
        }
        let since = last_event(&tx)?;
        let mut out: Vec<String> = header(&tx, pid, &b, now)?.into_iter().collect();
        for (i, p) in b.sections.iter().enumerate() {
            if b.sections[..i].iter().any(|q| q.key == p.key) {
                return Err(format!("rejected: the bill names section {} twice", p.key));
            }
            out.extend(section::apply(&tx, pid, p, now)?);
        }
        let mut seen = Vec::new();
        for l in &b.chunks {
            let line = match &l.uid {
                None => Some(add_line(&tx, pid, l, now)?),
                Some(uid) => {
                    let id = row(&tx, uid)?.id;
                    if seen.contains(&id) {
                        return Err(format!("rejected: the bill names chunk {uid} twice"));
                    }
                    seen.push(id);
                    edit_line(&tx, pid, uid, l, now)?
                }
            };
            out.extend(line);
        }
        for (i, op) in b.ops.into_iter().enumerate() {
            let line = run(&tx, op, now).map_err(|e| format!("op {}: {e}", i + 1))?;
            out.push(line);
        }
        let strays: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM event WHERE id > ?1 AND plan IS NOT ?2",
                params![since, pid],
                |r| r.get(0),
            )
            .map_err(err)?;
        if strays > 0 {
            return Err(
                "rejected: an op touches a chunk of another plan — a bill edits one plan".into(),
            );
        }
        batch(&tx, since)?;
        let new_rev = rev(&tx, pid)?;
        if !dry {
            tx.commit().map_err(err)?;
        }
        Ok((out, new_rev))
    }
}

#[cfg(test)]
#[path = "bill_tests.rs"]
mod tests;
