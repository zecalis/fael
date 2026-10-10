//! `fael plan import`: the markdown into plans.db. An `md` plan's chunks are synced in
//! place — each keeps its row and uid by a unique label, else a unique exact title
//! (SPEC §7) — and a `db` plan gets only its plan fields refreshed, never its chunks.

use super::md::{self, Tick};
use super::store::{Import, Report, Store, err};
use rusqlite::{Transaction, TransactionBehavior, params};
use std::collections::{HashMap, HashSet};

/// (chunk id, label, tick, raw line) — what the `(after …)` pass resolves against
type Chunks<'a> = Vec<(i64, Option<String>, Tick, &'a str)>;

impl Store {
    /// Sync every plan of the apps in `plans` with its markdown, in one transaction. An
    /// `md` plan of those apps whose file is gone is deleted; a `db` plan never is.
    pub fn import_all(&mut self, plans: &[Import], now: &str) -> Result<Report, String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let mut apps: Vec<&str> = plans.iter().map(|p| p.app.as_str()).collect();
        apps.sort_unstable();
        apps.dedup();
        let keep: HashSet<(&str, &str)> = plans
            .iter()
            .map(|p| (p.app.as_str(), p.md.name.as_str()))
            .collect();
        for app in &apps {
            let gone: Vec<(i64, String)> = tx
                .prepare("SELECT id, name FROM plan WHERE app = ?1 AND truth = 'md'")
                .map_err(err)?
                .query_map([app], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(err)?
                .collect::<Result<_, _>>()
                .map_err(err)?;
            for (id, name) in gone {
                if !keep.contains(&(*app, name.as_str())) {
                    tx.execute("DELETE FROM plan WHERE id = ?1", [id])
                        .map_err(err)?;
                }
            }
        }
        let mut rep = Report::default();
        let mut ids: HashMap<(&str, &str), (&str, Chunks)> = HashMap::new();
        let mut md_plans = Vec::new();
        for p in plans {
            let (plan, truth) = upsert_plan(&tx, p)?;
            rep.plans += 1;
            let chunks = if truth == "db" {
                rep.db_plans += 1;
                db_chunks(&tx, plan)?
            } else {
                md_plans.push(p);
                sync_chunks(&tx, plan, p, now, &mut rep)?
            };
            ids.insert((&p.app, &p.md.name), (&p.dir, chunks));
        }
        for p in md_plans {
            let (_, own) = &ids[&(p.app.as_str(), p.md.name.as_str())];
            let mut prev: Option<(i64, Option<String>)> = None;
            for (id, label, tick, raw) in own {
                tx.execute("DELETE FROM edge WHERE src = ?1 AND kind = 'after'", [id])
                    .map_err(err)?;
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
                            "INSERT INTO edge(src, dst, ref, kind) VALUES (?1, ?2, ?3, 'after')",
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

/// Insert or refresh the plan's own fields; its id and `truth` stay.
fn upsert_plan(tx: &Transaction, p: &Import) -> Result<(i64, String), String> {
    let f = &p.md.front;
    let state = match p.dir.as_str() {
        "done" => "done",
        "parked" => "parked",
        _ if f.status.as_deref() == Some("blocked") => "blocked",
        _ => "active",
    };
    tx.query_row(
        "INSERT INTO plan(app, name, title, area, kind, state, spec, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(app, name) DO UPDATE SET title = excluded.title, area = excluded.area,
           kind = excluded.kind, state = excluded.state, spec = excluded.spec,
           source = excluded.source
         RETURNING id, truth",
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
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(err)
}

/// A `db` plan's chunks, read only so an md plan's `(after plan:label)` can name them.
fn db_chunks(tx: &Transaction, plan: i64) -> Result<Chunks<'static>, String> {
    tx.prepare("SELECT id, label, state FROM chunk WHERE plan = ?1 ORDER BY seq")
        .map_err(err)?
        .query_map([plan], |r| {
            let tick = match r.get::<_, String>(2)?.as_str() {
                "draft" => Tick::Unknown,
                "done" => Tick::Done,
                "dropped" | "replaced" => Tick::Dropped,
                _ => Tick::Open,
            };
            Ok((r.get(0)?, r.get(1)?, tick, ""))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}

/// Write an md plan's chunks over its rows: a matched chunk keeps its row (and every edge
/// another plan points at it), a new one gets a new uid, a vanished one goes.
fn sync_chunks<'a>(
    tx: &Transaction,
    plan: i64,
    p: &'a Import,
    now: &str,
    rep: &mut Report,
) -> Result<Chunks<'a>, String> {
    type Old = (i64, String, Option<String>, String, String);
    let old: Vec<Old> = tx
        .prepare("SELECT id, uid, label, title, state FROM chunk WHERE plan = ?1 ORDER BY seq")
        .map_err(err)?
        .query_map([plan], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let keys = |l: &Option<String>, t: &String| (l.clone(), t.clone());
    let (hit, ambiguous) = match_old(
        &old.iter().map(|o| keys(&o.2, &o.3)).collect::<Vec<_>>(),
        &p.md
            .items
            .iter()
            .map(|it| keys(&it.label, &it.text))
            .collect::<Vec<_>>(),
    );
    for i in ambiguous {
        let it = &p.md.items[i];
        let what = it.label.as_deref().unwrap_or(&it.text);
        rep.ambiguous
            .push(format!("{}/{} {what}", p.app, p.md.name));
    }
    // free every seq first: UNIQUE(plan, seq) holds row by row
    tx.execute("UPDATE chunk SET seq = -seq WHERE plan = ?1", [plan])
        .map_err(err)?;
    let mut out = Vec::new();
    for (i, it) in p.md.items.iter().enumerate() {
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
        let seq = (i as i64 + 1) * 1024;
        let (id, uid, was) = match hit[i].map(|o| &old[o]) {
            Some((id, uid, _, _, was)) => {
                tx.execute(
                    "UPDATE chunk SET seq = ?2, label = ?3, title = ?4, state = ?5, brief = ?4,
                       wait_text = ?6 WHERE id = ?1",
                    params![id, seq, it.label, it.text, state, wait],
                )
                .map_err(err)?;
                (*id, uid.clone(), Some(was.as_str()))
            }
            None => {
                let uid = crate::ulid();
                tx.execute(
                    "INSERT INTO chunk(uid, plan, seq, label, title, state, brief, wait_text)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?7)",
                    params![uid, plan, seq, it.label, it.text, state, wait],
                )
                .map_err(err)?;
                (tx.last_insert_rowid(), uid, None)
            }
        };
        // an md plan's runs are only its (wip) markers: the run stays while its branch does
        let wip = md::marker(&it.raw, "wip");
        tx.execute(
            "DELETE FROM run WHERE chunk = ?1 AND (?2 IS NULL OR branch IS NOT ?2)",
            params![id, wip],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT INTO run(start, chunk, branch, started) SELECT ?1, ?2, ?3, ?4
             WHERE ?3 IS NOT NULL AND NOT EXISTS (SELECT 1 FROM run WHERE chunk = ?2)",
            params![crate::ulid(), id, wip, now],
        )
        .map_err(err)?;
        if was != Some(state) {
            tx.execute(
                "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text)
                 VALUES (?1, ?2, 'import', ?3, ?4, ?5)",
                params![uid, now, was, state, p.source],
            )
            .map_err(err)?;
        }
        rep.chunks += 1;
        rep.drafts += usize::from(it.tick == Tick::Unknown);
        out.push((id, it.label.clone(), it.tick, it.raw.as_str()));
    }
    // ponytail: a vanished chunk's row goes (its events stay, keyed on uid); an `after`
    // edge another plan held on it turns unresolved
    tx.execute("DELETE FROM chunk WHERE plan = ?1 AND seq < 0", [plan])
        .map_err(err)?;
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

/// (label, title) of one chunk — what a re-import matches a uid on
pub(super) type Key = (Option<String>, String);

fn key(x: &Key, by_label: bool) -> Option<&str> {
    if by_label { x.0.as_deref() } else { Some(&x.1) }
}

fn index(xs: &[Key], by_label: bool) -> HashMap<&str, Vec<usize>> {
    let mut m: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, x) in xs.iter().enumerate() {
        if let Some(k) = key(x, by_label) {
            m.entry(k).or_default().push(i);
        }
    }
    m
}

/// SPEC §7: which old chunk each new one is — by a label unique on both sides, then by
/// an exact title unique on both sides, never one old chunk twice. Also the new chunks
/// left without a match whose label or title the old side holds: ambiguous, not new.
pub(super) fn match_old(old: &[Key], new: &[Key]) -> (Vec<Option<usize>>, Vec<usize>) {
    let mut hit = vec![None; new.len()];
    let mut used = HashSet::new();
    let olds = [index(old, true), index(old, false)];
    for by_label in [true, false] {
        let (o, n) = (&olds[usize::from(!by_label)], index(new, by_label));
        for (i, x) in new.iter().enumerate() {
            if let Some(k) = key(x, by_label)
                && hit[i].is_none()
                && let (Some([j]), [_]) = (o.get(k).map(Vec::as_slice), n[k].as_slice())
                && used.insert(*j)
            {
                hit[i] = Some(*j);
            }
        }
    }
    let ambiguous = (0..new.len())
        .filter(|&i| {
            hit[i].is_none()
                && [true, false].iter().any(|&b| {
                    key(&new[i], b).is_some_and(|k| olds[usize::from(!b)].contains_key(k))
                })
        })
        .collect();
    (hit, ambiguous)
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
