//! `fael plan migrate` (plan fael-v8): once per repo, every plan's md and every spec md
//! become sections, md plans hand their chunk list to the db, and the report names the
//! files now safe to delete — it deletes none. One `BEGIN IMMEDIATE`; before the commit a
//! copy of the db as it stood (`<db>.pre-migrate.bak`), or nothing is written.

use super::export::prose;
use super::schema::copy;
use super::section::{put, split};
use super::store::{Store, err};
use rusqlite::{Transaction, TransactionBehavior, params};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::PathBuf;

/// A spec md as the caller read it: `app` = its `.fapony/`'s folder (`""` at the root),
/// `path` repo-relative (`.fapony/spec/SPEC-x.md`).
pub struct Spec {
    pub app: String,
    pub path: String,
    pub text: String,
}

struct Plan {
    id: i64,
    app: String,
    name: String,
    state: String,
}

impl Plan {
    fn key(&self) -> String {
        if self.app.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.app, self.name)
        }
    }
    fn open(&self) -> bool {
        matches!(self.state.as_str(), "active" | "blocked")
    }
}

/// What one plan got: report lines and the files it copied.
#[derive(Default)]
struct Got {
    notes: Vec<String>,
    safe: Vec<String>,
}

impl Store {
    /// `md` reads a repo-relative file, `None` when it is gone. Returns the report; `dry`
    /// rolls it all back, so its counts are the real run's.
    pub fn migrate_md(
        &mut self,
        md: &dyn Fn(&str) -> Option<String>,
        specs: &[Spec],
        dry: bool,
    ) -> Result<String, String> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(err)?;
        let now = crate::rfc3339(crate::now_ms());
        let rows: Vec<(Plan, String, String, i64)> = tx
            .prepare(
                "SELECT id, app, name, state, truth, source,
                   (SELECT COUNT(*) FROM section s WHERE s.plan = plan.id)
                 FROM plan ORDER BY app, name",
            )
            .map_err(err)?
            .query_map([], |r| {
                Ok((
                    Plan {
                        id: r.get(0)?,
                        app: r.get(1)?,
                        name: r.get(2)?,
                        state: r.get(3)?,
                    },
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let mut got: BTreeMap<String, Got> = BTreeMap::new();
        let (mut sections, mut flipped, mut held) = (0, 0, Vec::new());
        for (p, truth, source, has) in &rows {
            let g = got.entry(p.key()).or_default();
            if truth == "md" && p.open() {
                g.notes.push(format!(
                    "skipped — its chunks still live in its md: `fael plan cutover {}` first",
                    p.key()
                ));
                held.push(p.id);
                continue;
            }
            if *has > 0 {
                g.notes.push(format!("md skipped — has {has} sections"));
            } else if source.is_empty() {
                g.notes.push("no md".into());
            } else if let Some(text) = md(source) {
                let n = write(&tx, p.id, "", &plan_text(&text), &now)?;
                sections += n;
                g.notes.push(format!("{source}: {n} sections"));
                g.safe.push(source.clone());
            } else {
                g.notes.push(format!("md skipped — {source} unreadable"));
            }
            if truth == "md" {
                tx.execute("UPDATE plan SET truth = 'db' WHERE id = ?1", [p.id])
                    .map_err(err)?;
                flipped += 1;
                g.notes.push("truth md → db".into());
            }
        }
        let plans: Vec<&Plan> = rows.iter().map(|r| &r.0).collect();
        let (n, attached, orphans) = attach(&tx, &plans, &held, specs, &mut got, &now)?;
        sections += n;
        let r = super::migrate_reads::run(&tx, &now)?;
        let mut o = String::new();
        for (k, g) in &got {
            let _ = writeln!(o, "{k}: {}", g.notes.join(" · "));
            if !g.safe.is_empty() {
                let _ = writeln!(o, "  safe to delete: {}", g.safe.join(" "));
            }
        }
        for u in &orphans {
            let _ = writeln!(o, "unattached: {u}");
        }
        o.push_str(&r.lines.concat());
        let _ = write!(
            o,
            "migrate: {} plans, {sections} sections, {attached} specs attached, {} unattached, \
             {flipped} md plans → truth db, {}",
            rows.len(),
            orphans.len(),
            r.counts()
        );
        let db = tx.path().filter(|p| !p.is_empty()).map(PathBuf::from);
        if dry || sections + flipped + r.writes == 0 {
            o.push_str(if dry {
                " — dry run, nothing written"
            } else {
                " — nothing to write"
            });
            return Ok(o);
        }
        if let Some(db) = db {
            let to = copy(&db, "pre-migrate")
                .map_err(|e| format!("rejected: migrate refused: {e}; nothing changed"))?;
            let _ = write!(o, " — backup {}", to.display());
        }
        tx.commit().map_err(err)?;
        Ok(o)
    }
}

/// Each spec's sections onto its plan ([`owner`]), keys `<x>:<key>`; a spec whose plan
/// already has one of them is skipped. Returns (sections, specs attached, unattached).
fn attach(
    tx: &Transaction,
    plans: &[&Plan],
    held: &[i64],
    specs: &[Spec],
    got: &mut BTreeMap<String, Got>,
    now: &str,
) -> Result<(usize, usize, Vec<String>), String> {
    let (mut sections, mut attached, mut orphans) = (0, 0, Vec::new());
    for s in specs {
        let Some((p, why)) = owner(plans, s) else {
            orphans.push(format!(
                "{} — names no plan of this repo (used by: {}), and no plan '{}'",
                s.path,
                or_none(&used_by(&s.text)),
                spec_name(&s.path)
            ));
            continue;
        };
        let g = got.entry(p.key()).or_default();
        let x = spec_name(&s.path);
        if held.contains(&p.id) {
            g.notes.push(format!("{} skipped with the plan", s.path));
            continue;
        }
        let prefix = format!("{x}:");
        let there: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM section WHERE plan = ?1
                   AND substr(key, 1, length(?2)) = ?2)",
                params![p.id, prefix],
                |r| r.get(0),
            )
            .map_err(err)?;
        if there {
            g.notes
                .push(format!("{} skipped — {prefix}* already there", s.path));
            continue;
        }
        let n = write(tx, p.id, &prefix, &split(&s.text), now)?;
        sections += n;
        attached += 1;
        g.notes.push(format!("{} ({why}): {n} sections", s.path));
        g.safe.push(s.path.clone());
    }
    Ok((sections, attached, orphans))
}

/// An md plan's text as sections: what export prints after the chunks (its TL;DR prose
/// lands before the first heading, so it becomes section `tldr`, titled apart from the
/// chunk list's `## TL;DR`).
fn plan_text(md: &str) -> Vec<(String, String, String)> {
    let mut secs = split(&prose(md));
    if let Some(s) = secs.first_mut().filter(|s| s.0 == "intro") {
        *s = ("tldr".into(), "Summary".into(), std::mem::take(&mut s.2));
    }
    secs
}

fn write(
    tx: &Transaction,
    pid: i64,
    prefix: &str,
    secs: &[(String, String, String)],
    now: &str,
) -> Result<usize, String> {
    for (key, title, body) in secs {
        put(
            tx,
            pid,
            &format!("{prefix}{key}"),
            Some(title),
            Some(body),
            now,
        )?;
    }
    Ok(secs.len())
}

/// `SPEC-x.md` → `x`, as a plan name is (lowercased).
fn spec_name(path: &str) -> String {
    let f = path.rsplit('/').next().unwrap_or(path);
    let f = f.strip_suffix(".md").unwrap_or(f);
    f.get(5..)
        .filter(|_| f[..5].eq_ignore_ascii_case("SPEC-"))
        .unwrap_or(f)
        .to_lowercase()
}

/// The plan names on the spec's `> **Used by:**` line, in order (`PLAN-x.md`, `PLAN-x`).
fn used_by(text: &str) -> Vec<String> {
    let Some(line) = text
        .lines()
        .find(|l| l.trim_start().starts_with('>') && l.to_lowercase().contains("used by"))
    else {
        return Vec::new();
    };
    line.match_indices("PLAN-")
        .map(|(i, _)| {
            let rest = &line[i + 5..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || "-_.".contains(c)))
                .unwrap_or(rest.len());
            let n = rest[..end].trim_end_matches('.');
            n.strip_suffix(".md").unwrap_or(n).to_lowercase()
        })
        .filter(|n| !n.is_empty())
        .collect()
}

fn or_none(v: &[String]) -> String {
    if v.is_empty() {
        "none".into()
    } else {
        v.join(", ")
    }
}

/// The plan a spec belongs to, in its own app: the first open plan its Used by line names,
/// else the first named one, else the plan named like the spec. ponytail: never a guess
/// beyond that — the rest is reported unattached.
fn owner<'a>(plans: &[&'a Plan], s: &Spec) -> Option<(&'a Plan, &'static str)> {
    let find = |n: &str| {
        plans
            .iter()
            .copied()
            .find(|p| p.app == s.app && p.name == n)
    };
    let named: Vec<&Plan> = used_by(&s.text).iter().filter_map(|n| find(n)).collect();
    named
        .iter()
        .find(|p| p.open())
        .or(named.first())
        .map(|p| (*p, "used by"))
        .or_else(|| find(&spec_name(&s.path)).map(|p| (p, "by name")))
}

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
