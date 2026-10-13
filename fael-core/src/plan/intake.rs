//! `fael intake` (plan fael-intake, decision 01M4TKVY): tickets an app's adapter lists become
//! inbox chunks. The CLI runs `list`; this parses `.fael/sources.json` and the JSONL, and
//! writes one run in one `BEGIN IMMEDIATE` — dedup by source+ref+reopen in every plan,
//! adoption of a chunk an older sync made, the brief. Fael never writes to the source.

use super::chunk::{Fields, row, set, tx};
use super::chunk_add::add_in;
use super::store::{Store, err};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One entry of `.fael/sources.json`: the adapter's commands, `{ref}` filled per ticket.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    #[serde(skip)]
    pub name: String,
    pub list: String,
    pub read: String,
    pub draft: String,
    pub open: String,
    #[serde(default)]
    claim: Option<Claim>,
}

/// One command for every kind, or one per kind (a kind left out is never claimed).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Claim {
    All(String),
    ByKind(BTreeMap<String, String>),
}

/// One line of `list`: exactly these fields (strict keeps customer data out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub r#ref: String,
    pub kind: String,
    pub title: String,
    pub reopen: u32,
    pub closed: Option<String>,
}

/// `chunk.ticket` on `fael board --json`; `open` is the source's url with `{ref}` filled,
/// which the CLI sets from `.fael/sources.json` (null when the source is gone).
#[derive(Debug, Clone, Serialize)]
pub struct Ticket {
    pub source: String,
    #[serde(rename = "ref")]
    pub what: String,
    pub kind: String,
    pub reopen: i64,
    pub open: Option<String>,
}

/// `.fael/sources.json`, by name.
pub fn sources(json: &str) -> Result<Vec<Source>, String> {
    let m: BTreeMap<String, Source> =
        serde_json::from_str(json).map_err(|e| format!(".fael/sources.json: {e}"))?;
    Ok(m.into_iter()
        .map(|(name, s)| Source { name, ..s })
        .collect())
}

/// Every line of `list`'s stdout, all checked before anything is written. Err names the line.
pub fn listed(out: &str) -> Result<Vec<Listed>, String> {
    let mut v = Vec::new();
    for (i, line) in out.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        v.push(one(line).map_err(|e| format!("list line {}: {e}", i + 1))?);
    }
    Ok(v)
}

fn one(line: &str) -> Result<Listed, String> {
    let serde_json::Value::Object(m) =
        serde_json::from_str(line).map_err(|e| format!("not JSON ({e})"))?
    else {
        return Err("not a JSON object".into());
    };
    const KEYS: [&str; 5] = ["ref", "kind", "title", "reopen", "closed"];
    if let Some(k) = m.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!(
            "field {k:?} is not in the contract ({})",
            KEYS.join(", ")
        ));
    }
    if let Some(k) = KEYS.iter().find(|k| !m.contains_key(**k)) {
        return Err(format!("field {k:?} is missing"));
    }
    let word = |k: &str| match m[k].as_str() {
        Some(s) if !s.is_empty() && !s.contains(char::is_whitespace) => Ok(s.to_string()),
        _ => Err(format!("{k} must be one word")),
    };
    let title: String = match m["title"].as_str().map(str::trim) {
        Some(t) if !t.is_empty() => t.chars().take(100).collect(),
        _ => return Err("title must be a non-empty string".into()),
    };
    let reopen = m["reopen"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("reopen must be an integer >= 0")?;
    let closed = match &m["closed"] {
        serde_json::Value::Null => None,
        serde_json::Value::String(c) if c == "closed" || c.len() > 4 && c.starts_with("dup:") => {
            Some(c.clone())
        }
        _ => return Err("closed must be null, \"closed\" or \"dup:<ref>\"".into()),
    };
    Ok(Listed {
        r#ref: word("ref")?,
        kind: word("kind")?,
        title,
        reopen,
        closed,
    })
}

/// `lower(ref)`, `-rN` for reopen round N.
fn label(t: &Listed) -> String {
    let l = t.r#ref.to_lowercase();
    match t.reopen {
        0 => l,
        n => format!("{l}-r{n}"),
    }
}

/// The brief of a ticket's chunk (the contract's template): fixed keys first, the Done block
/// for its kind, a Claim line only when the source claims that kind.
pub fn brief(s: &Source, t: &Listed) -> String {
    let fill = |c: &str| c.replace("{ref}", &t.r#ref);
    let r = &t.r#ref;
    let mut b = format!(
        "Ticket: {r} ({})\nKind: {}\nRead: {}\nOpen: {}\n",
        s.name,
        t.kind,
        fill(&s.read),
        fill(&s.open)
    );
    let claim = match &s.claim {
        Some(Claim::All(c)) => Some(c),
        Some(Claim::ByKind(m)) => m.get(&t.kind),
        None => None,
    };
    // a question is never claimed: the user would read "being fixed"
    if let Some(c) = claim.filter(|_| t.kind != "question") {
        let when = match t.kind.as_str() {
            "report" => " — once you know it is a bug; a how-to is never claimed",
            _ => "",
        };
        b += &format!("Claim: {}{when}\n", fill(c));
    }
    let done = match t.kind.as_str() {
        "bug" => format!(
            "fix + a guard test, push pr, the commit names {r}; fael chunk done --pr N --verdict fix"
        ),
        "report" => "a bug: as bug (fix + a guard test, push pr, done --pr N --verdict fix) · a how-to: Draft the answer, done --verdict answer · not doing it: Draft why, done --verdict decline".into(),
        "question" => "Draft the answer, done --verdict answer · turns out a bug: fix + a guard test, push pr, done --pr N --verdict fix".into(),
        "idea" => "Draft what you propose and why, done --verdict consider|plan|decline; never build it unasked".into(),
        _ => "Read it, then: a code fix as a bug (fix + a guard test, push pr, done --pr N --verdict fix), else Draft a reply and done --verdict answer|decline".into(),
    };
    b += &format!(
        "Draft: {} \"…\" — a person sends it from Open\nDone: {done}\nNever send: a person sends every word the user reads.",
        fill(&s.draft)
    );
    b
}

/// The chunk this ticket round already has (any plan, any state).
fn ticketed(tx: &Transaction, s: &str, t: &Listed) -> Result<Option<String>, String> {
    tx.query_row(
        "SELECT uid FROM chunk WHERE ticket_source = ?1 AND ticket_ref = ?2 AND ticket_reopen = ?3",
        params![s, t.r#ref, t.reopen],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
}

fn stamp(tx: &Transaction, uid: &str, s: &str, t: &Listed) -> Result<(), String> {
    tx.execute(
        "UPDATE chunk SET ticket_source = ?1, ticket_ref = ?2, ticket_kind = ?3, ticket_reopen = ?4
         WHERE uid = ?5",
        params![s, t.r#ref, t.kind, t.reopen, uid],
    )
    .map(drop)
    .map_err(err)
}

/// A chunk an older sync made (label `lower(ref)[-rN]`, no ticket, any plan): the oldest not
/// dropped or replaced takes the ticket, the rest are named. Empty = none.
fn adopt(tx: &Transaction, s: &str, t: &Listed, now: &str) -> Result<Vec<String>, String> {
    let uids: Vec<String> = tx
        .prepare(
            "SELECT uid FROM chunk WHERE label = ?1 AND ticket_ref IS NULL
             ORDER BY state IN ('dropped', 'replaced'), id",
        )
        .map_err(err)?
        .query_map([label(t)], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let Some((first, rest)) = uids.split_first() else {
        return Ok(Vec::new());
    };
    stamp(tx, first, s, t)?;
    let r = row(tx, first)?;
    let text = format!("adopted ticket {} ({s})", t.r#ref);
    set(tx, &r, r.state, "intake", Some(&text), now)?;
    let mut out = vec![format!("adopted {} → {first}", t.r#ref)];
    out.extend(
        rest.iter()
            .map(|u| format!("not adopted {u} ({})", t.r#ref)),
    );
    Ok(out)
}

impl Store {
    /// One intake run over every source's checked list, in one `BEGIN IMMEDIATE`: a second run
    /// waits on the write lock and finds what this one wrote. Returns one line per change.
    pub fn intake(
        &mut self,
        runs: &[(Source, Vec<Listed>)],
        now: &str,
    ) -> Result<Vec<String>, String> {
        let tx = tx(self)?;
        let mut out = Vec::new();
        for (s, list) in runs {
            for t in list {
                if ticketed(&tx, &s.name, t)?.is_some() {
                    continue;
                }
                let adopted = adopt(&tx, &s.name, t, now)?;
                if !adopted.is_empty() {
                    out.extend(adopted);
                    continue;
                }
                if t.closed.is_some() {
                    continue;
                }
                let f = Fields {
                    title: Some(t.title.clone()),
                    brief: Some(brief(s, t)),
                    size: Some("S".into()),
                    label: Some(label(t)),
                    ..Fields::default()
                };
                let (_, uid) = add_in(&tx, "inbox", &f, &[], "intake", now)?;
                stamp(&tx, &uid, &s.name, t)?;
                out.push(format!("added {} → {uid}", t.r#ref));
            }
        }
        tx.commit().map_err(err)?;
        Ok(out)
    }
}

#[cfg(test)]
#[path = "intake_tests.rs"]
mod tests;
