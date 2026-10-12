//! `fael plan migrate`, once the md and specs are sections (plan fael-v8 `migrate-reads`):
//! an open chunk that cites `SPEC-<x> §<n>` reads that section, a link to a spec or plan md
//! names where its text lives now, and a section that is a code scan is reported for the
//! owner to drop. ponytail: never a guess — a bare `§<n>` resolves only in a plan that holds
//! exactly one spec; anything else is reported and left alone.

use super::section::put;
use super::store::err;
use rusqlite::{Transaction, params};

/// One spec's sections as migrate wrote them: keys `<x>:<key>` in plan `pid`.
struct Spec {
    app: String,
    plan: String,
    pid: i64,
    x: String,
    rows: Vec<(String, String)>,
}

/// What the pass did: report lines, rows it wrote, and the counts for the summary.
#[derive(Default)]
pub(super) struct Done {
    pub(super) lines: Vec<String>,
    pub(super) writes: usize,
    pub(super) reads: usize,
    pub(super) ambiguous: usize,
    pub(super) missing: usize,
    pub(super) relinked: usize,
    pub(super) scans: usize,
}

impl Done {
    fn line(&mut self, l: String) {
        self.lines.push(l + "\n");
    }

    /// The summary's tail: what this pass did.
    pub(super) fn counts(&self) -> String {
        format!(
            "{} chunks given reads ({} ambiguous, {} missing), {} sections relinked, \
             {} code-scan sections",
            self.reads, self.ambiguous, self.missing, self.relinked, self.scans
        )
    }
}

pub(super) fn run(tx: &Transaction, now: &str) -> Result<Done, String> {
    let held = specs(tx)?;
    let mut d = Done::default();
    relink_all(tx, &held, now, &mut d)?;
    fill(tx, &held, now, &mut d)?;
    scans(tx, &mut d)?;
    Ok(d)
}

fn specs(tx: &Transaction) -> Result<Vec<Spec>, String> {
    let rows: Vec<(i64, String, String, String, String)> = tx
        .prepare(
            "SELECT s.plan, p.app, p.name, s.key, s.title FROM section s
             JOIN plan p ON p.id = s.plan WHERE instr(s.key, ':') > 0 ORDER BY s.plan, s.seq",
        )
        .map_err(err)?
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut out: Vec<Spec> = Vec::new();
    for (pid, app, name, key, title) in rows {
        let x = key.split(':').next().unwrap_or_default().to_string();
        let i = match out.iter().position(|s| s.pid == pid && s.x == x) {
            Some(i) => i,
            None => {
                let plan = if app.is_empty() {
                    name
                } else {
                    format!("{app}/{name}")
                };
                out.push(Spec {
                    app,
                    plan,
                    pid,
                    x,
                    rows: Vec::new(),
                });
                out.len() - 1
            }
        };
        out[i].rows.push((key, title));
    }
    Ok(out)
}

/// The section `§<n>` of a spec: the one whose heading starts `§<n>`, else, for `§17.4`,
/// the `§17` it sits in.
fn key_in(s: &Spec, n: &str) -> Option<String> {
    let head = |t: &str| {
        t.split_whitespace()
            .next()
            .map(|w| w.trim_end_matches(['.', ':', ')']).to_string())
    };
    let find = |n: &str| {
        s.rows
            .iter()
            .find(|(_, t)| head(t).as_deref() == Some(&format!("§{n}")))
            .map(|(k, _)| k.clone())
    };
    find(n).or_else(|| n.split_once('.').and_then(|(p, _)| find(p)))
}

/// A ref as the brief resolves it from plan `here`: the bare key in its own plan.
fn refer(s: &Spec, here: i64, key: &str) -> String {
    if s.pid == here {
        key.to_string()
    } else {
        format!("{}/{key}", s.plan)
    }
}

/// Every `§<n>` in `text` once, with the `x` of a `SPEC-<x>` (or `SPEC-<x>.md`) right before it.
fn cites(text: &str) -> Vec<(Option<String>, String)> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices('§') {
        let rest = &text[i + '§'.len_utf8()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
            .unwrap_or(rest.len());
        let n = rest[..end].trim_end_matches('.');
        if n.is_empty() {
            continue;
        }
        let before = text[..i].trim_end();
        let before = before.strip_suffix(".md").unwrap_or(before);
        let start = before
            .char_indices()
            .rev()
            .find(|&(_, c)| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .map_or(0, |(j, c)| j + c.len_utf8());
        let tok = &before[start..];
        let x = (tok.len() > 5 && tok[..5].eq_ignore_ascii_case("SPEC-"))
            .then(|| tok[5..].to_lowercase());
        let c = (x, n.to_string());
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Open chunks with no reads, in plans whose chunks are rows: each cited section becomes a
/// read. A chunk with reads already is skipped, so a second run writes nothing.
fn fill(tx: &Transaction, held: &[Spec], now: &str, d: &mut Done) -> Result<(), String> {
    let rows: Vec<(i64, String, String, String, String, i64, String)> = tx
        .prepare(
            "SELECT c.id, c.uid, c.state, c.title, c.brief, c.plan, p.app FROM chunk c
             JOIN plan p ON p.id = c.plan
             WHERE c.state IN ('draft', 'open', 'waiting', 'parked')
               AND COALESCE(c.reads, '') = '' AND p.truth <> 'md'
             ORDER BY c.plan, c.seq",
        )
        .map_err(err)?
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    for (id, uid, state, title, brief, pid, app) in rows {
        let mut refs: Vec<String> = Vec::new();
        for (x, n) in cites(&format!("{title}\n{brief}")) {
            let spec = match &x {
                Some(x) => held.iter().find(|s| s.app == app && s.x == *x),
                None => {
                    let mine: Vec<&Spec> = held.iter().filter(|s| s.pid == pid).collect();
                    if mine.len() != 1 {
                        d.ambiguous += 1;
                        d.line(format!(
                            "ambiguous: {uid} §{n} — its plan holds {} specs",
                            mine.len()
                        ));
                        continue;
                    }
                    Some(mine[0])
                }
            };
            let Some((s, key)) = spec.and_then(|s| key_in(s, &n).map(|k| (s, k))) else {
                d.missing += 1;
                let x = x.map_or(String::new(), |x| format!("SPEC-{x} "));
                d.line(format!("missing: {uid} {x}§{n} — no such section"));
                continue;
            };
            let r = refer(s, pid, &key);
            if !refs.contains(&r) {
                refs.push(r);
            }
        }
        if refs.is_empty() {
            continue;
        }
        tx.execute(
            "UPDATE chunk SET reads = ?1 WHERE id = ?2",
            params![refs.join("\n"), id],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
             VALUES (?1, ?2, 'owner', ?3, ?3, ?4, ?5)",
            params![
                uid,
                now,
                state,
                format!("reads set by plan migrate: {}", refs.join(", ")),
                pid
            ],
        )
        .map_err(err)?;
        d.reads += 1;
        d.writes += 1;
        d.line(format!("reads: {uid} ← {}", refs.join(", ")));
    }
    Ok(())
}

/// Every section whose body links to a spec or plan md: the link names where it lives now.
fn relink_all(tx: &Transaction, held: &[Spec], now: &str, d: &mut Done) -> Result<(), String> {
    let rows: Vec<(i64, String, String, String)> = tx
        .prepare(
            "SELECT s.plan, p.app, s.key, s.body FROM section s JOIN plan p ON p.id = s.plan
             WHERE s.body LIKE '%.md)%' OR s.body LIKE '%.md#%'",
        )
        .map_err(err)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    for (pid, app, key, body) in rows {
        let new = relink(&body, &app, pid, held);
        if new != body && put(tx, pid, &key, None, Some(&new), now)?.is_some() {
            d.relinked += 1;
            d.writes += 1;
        }
    }
    Ok(())
}

/// `[t](…SPEC-x.md…)` → `t (`<ref>`)` when `t` cites a section of x, else
/// `t (spec `x`, plan `p`)`; `[t](…PLAN-p.md)` → `plan `p``. A spec migrate did not attach
/// keeps its link (it is reported unattached).
fn relink(body: &str, app: &str, here: i64, held: &[Spec]) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(j) = rest.find("](") {
        let (Some(k), Some(m)) = (rest[..j].rfind('['), rest[j + 2..].find(')')) else {
            break;
        };
        let (text, target) = (&rest[k + 1..j], &rest[j + 2..j + 2 + m]);
        let file = target.split('#').next().unwrap_or_default();
        let file = file.rsplit('/').next().unwrap_or(file);
        let name = file.strip_suffix(".md").map(|f| (f.get(..5), f.get(5..)));
        let swap = match name {
            Some((Some(p), Some(x))) if p.eq_ignore_ascii_case("SPEC-") => {
                let x = x.to_lowercase();
                held.iter().find(|s| s.app == app && s.x == x).map(|s| {
                    let sec = cites(text).into_iter().find_map(|(_, n)| key_in(s, &n));
                    match sec {
                        Some(key) => format!("{text} (`{}`)", refer(s, here, &key)),
                        None => format!("{text} (spec `{x}`, plan `{}`)", s.plan),
                    }
                })
            }
            Some((Some(p), Some(n))) if p.eq_ignore_ascii_case("PLAN-") => {
                let n = n.to_lowercase();
                Some(
                    if text.eq_ignore_ascii_case(file)
                        || text.eq_ignore_ascii_case(&file[..file.len() - 3])
                    {
                        format!("plan `{n}`")
                    } else {
                        format!("{text} (plan `{n}`)")
                    },
                )
            }
            _ => None,
        };
        let end = j + 2 + m + 1;
        out.push_str(&rest[..k]);
        out.push_str(&swap.unwrap_or_else(|| rest[k..end].to_string()));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// A heading that marks a scan of the code (`Code anchors`, `Appendix`, a source path):
/// the code is its source and the copy rots, so the owner is asked to drop it.
fn scan(title: &str) -> bool {
    let t = title.rsplit("</a>").next().unwrap_or(title).trim();
    let l = t.to_lowercase();
    l.starts_with("code anchor")
        || l.starts_with("appendix")
        || t.split_whitespace().next().is_some_and(|w| {
            w.contains('/') && w.starts_with(|c: char| c.is_ascii_lowercase() || c == '.')
        })
}

fn scans(tx: &Transaction, d: &mut Done) -> Result<(), String> {
    let rows: Vec<(String, String, String, String, String)> = tx
        .prepare(
            "SELECT p.app, p.name, s.key, s.title, s.body FROM section s
             JOIN plan p ON p.id = s.plan WHERE p.state IN ('active', 'blocked')
             ORDER BY p.app, p.name, s.seq",
        )
        .map_err(err)?
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    for (app, name, key, title, body) in rows {
        if !scan(&title) {
            continue;
        }
        let plan = if app.is_empty() {
            name
        } else {
            format!("{app}/{name}")
        };
        d.scans += 1;
        d.line(format!(
            "code-scan: {plan} {key} ({} lines)",
            body.lines().count()
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "migrate_reads_tests.rs"]
mod tests;
