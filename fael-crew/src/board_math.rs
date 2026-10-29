//! The board's derived math (SPEC-fael-board §2): pair and transitive `unblocks` over the
//! edges, scope `overlaps`, and the md heading and chunk line read back as clean titles.

use super::State;
use super::board::Chunk;
use super::shared::unshared;
use super::store::err;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// pair, unblocks (transitive, over chunks not yet closed), overlaps (see `overlaps`).
pub(super) fn edges(conn: &Connection, ids: &[i64], chunks: &mut [Chunk]) -> Result<(), String> {
    let at: HashMap<i64, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut waits_on_me: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut q = conn
        .prepare("SELECT src, dst, kind FROM edge WHERE dst IS NOT NULL AND kind != 'replaced_by'")
        .map_err(err)?;
    for row in q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(err)?
    {
        let (src, dst, kind): (i64, i64, String) = row.map_err(err)?;
        let (Some(&s), Some(&d)) = (at.get(&src), at.get(&dst)) else {
            continue;
        };
        if kind == "pair" {
            let (su, du) = (chunks[s].uid.clone(), chunks[d].uid.clone());
            chunks[s].pair.push(du);
            chunks[d].pair.push(su);
        } else {
            waits_on_me.entry(d).or_default().push(s);
        }
    }
    let live = |c: &Chunk| !State::parse(&c.state).is_some_and(State::terminal);
    for i in 0..chunks.len() {
        let mut seen = HashSet::new();
        let mut todo = vec![i];
        while let Some(j) = todo.pop() {
            for &w in waits_on_me.get(&j).into_iter().flatten() {
                if seen.insert(w) {
                    todo.push(w);
                }
            }
        }
        chunks[i].unblocks = seen.into_iter().filter(|&j| live(&chunks[j])).count();
    }
    overlaps(chunks, &HashSet::new(), &[]);
    Ok(())
}

/// Ready, live running, review and `open_pr` chunks (a done chunk whose PR is not on the
/// base yet: the CLI reads git) whose scopes share a path prefix — their files are taken.
/// A `shared` path (`.fael/shared`) is nobody's: it never makes an overlap.
/// A static `pair` is one run's work, and a live joint run (`run.joint/partner`,
/// 01M5PE238) agreed to work beside its partner — neither names an overlap.
pub fn overlaps(chunks: &mut [Chunk], open_pr: &HashSet<String>, shared: &[String]) {
    for c in chunks.iter_mut() {
        c.overlaps.clear();
        c.overlaps_how.clear();
        c.overlap_classes.clear();
    }
    // a live joint run's partner uid, both directions: agreeing to share files
    // is no clash. Ended runs are gone — their overlap comes back.
    let mut joint: HashSet<(String, String)> = HashSet::new();
    for c in chunks.iter() {
        let live = c.run.as_ref().is_some_and(|r| r.ended.is_none());
        if live
            && let (Some(p), Some(_)) = (
                c.run.as_ref().and_then(|r| r.partner.clone()),
                c.run.as_ref().and_then(|r| r.joint.clone()),
            )
        {
            joint.insert((c.uid.clone(), p.clone()));
            joint.insert((p, c.uid.clone()));
        }
    }
    let hot: Vec<usize> = (0..chunks.len())
        .filter(|&i| {
            let c = &chunks[i];
            c.ready
                || c.state == "review"
                || (c.state == "running" && !c.ended)
                || open_pr.contains(&c.uid)
        })
        .collect();
    let own: Vec<Vec<String>> = chunks.iter().map(|c| unshared(&c.scope, shared)).collect();
    for &a in &hot {
        for &b in &hot {
            if a == b {
                continue;
            }
            // one run's work shares files on purpose: static pairs and live
            // joint partners never name each other
            if chunks[a].pair.contains(&chunks[b].uid) {
                continue;
            }
            if joint.contains(&(chunks[a].uid.clone(), chunks[b].uid.clone())) {
                continue;
            }
            // the class is Some exactly when the scopes overlap, so one call
            // fills the uid list (today's shape), the per-pair class and the
            // app's `overlaps_how` (who + class + witness path)
            let how: Option<(OverlapClass, String)> = overlap_how(&own[a], &own[b]);
            if let Some((class, path)) = how {
                let u = chunks[b].uid.clone();
                chunks[a].overlaps.push(u.clone());
                chunks[a].overlaps_how.push(super::board::OverlapHow {
                    uid: u.clone(),
                    class: class.as_str(),
                    path,
                });
                chunks[a].overlap_classes.push((u, class));
            }
        }
    }
}

pub use fael_core::{OverlapClass, overlap, overlap_how};
pub(super) use fael_core::{under, within};

/// `# PLAN-x — title` as imported → `title` (also `PLAN-x: title`, `PLAN-x (v1): title`).
pub(super) fn plan_title(title: &str, name: &str) -> String {
    let Some(rest) = title.strip_prefix(&format!("PLAN-{name}")) else {
        return title.to_string();
    };
    let rest = [" — ", " – ", ": "]
        .iter()
        .filter_map(|s| rest.find(s).map(|i| &rest[i + s.len()..]))
        .next()
        .unwrap_or(rest);
    rest.trim().to_string()
}

/// An md chunk line as imported → its title: the leading `label — ` and the `(wip …)`,
/// `(wait …)`, `(after …)` markers go; a db chunk added with a clean title is unchanged.
pub(super) fn clean(title: &str, label: Option<&str>) -> String {
    let mut t = title.to_string();
    for word in ["wip", "wait", "after"] {
        // as `md::marker` reads it: `(<word>` ending at a non-word char, closed by `)`
        let pat = format!("({word}");
        let mut from = 0;
        while let Some(i) = t.to_ascii_lowercase()[from..].find(&pat).map(|i| i + from) {
            let at = i + pat.len();
            let whole = t[at..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
            match t[at..].find(')') {
                Some(end) if whole => t.replace_range(i..=at + end, ""),
                _ => from = at,
            }
        }
    }
    let mut s = t.trim();
    if let Some(l) = label {
        let r = s.strip_prefix("chunk ").unwrap_or(s);
        if let Some(r) = r.strip_prefix(l).map(str::trim_start)
            && let Some(r) = r.strip_prefix(['—', '–'])
        {
            s = r.trim_start();
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
