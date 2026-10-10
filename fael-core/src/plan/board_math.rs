//! The board's derived math (SPEC-fael-board §2): pair and transitive `unblocks` over the
//! edges, scope `overlaps`, and the md heading and chunk line read back as clean titles.

use super::State;
use super::board::Chunk;
use super::store::err;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// pair, unblocks (transitive, over chunks not yet closed), overlaps (ready or live
/// running chunks whose scopes share a path prefix).
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
    let hot: Vec<usize> = (0..chunks.len())
        .filter(|&i| {
            let c = &chunks[i];
            c.ready || (c.state == "running" && !c.ended)
        })
        .collect();
    for &a in &hot {
        for &b in &hot {
            if a != b && overlap(&chunks[a].scope, &chunks[b].scope) {
                let u = chunks[b].uid.clone();
                chunks[a].overlaps.push(u);
            }
        }
    }
    Ok(())
}

/// SPEC §2: two scopes overlap when a path of one is a prefix of a path of the other on a
/// `/` boundary (`src/` covers `src/a.rs`, `src/a` does not cover `src/ab.rs`).
pub fn overlap(a: &[String], b: &[String]) -> bool {
    let under = |x: &str, y: &str| {
        let y = y.trim_end_matches('/');
        x == y || x.strip_prefix(y).is_some_and(|r| r.starts_with('/'))
    };
    a.iter().any(|x| {
        b.iter().any(|y| {
            let (x, y) = (x.trim_end_matches('/'), y.as_str());
            under(x, y) || under(y.trim_end_matches('/'), x)
        })
    })
}

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
