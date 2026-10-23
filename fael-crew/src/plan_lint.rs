//! `fael plan lint <plan>` (issue 01M5KVYE): section bodies cite H-refs
//! (`H4`, `H4-H8`) with no chunk row — the vela-house-books drift where the
//! text names work the db never cut. One read, no write.

use super::section::Row;
use super::store::{Store, err};
use std::collections::{BTreeMap, HashSet};

/// One cited ref with no chunk row, and the section keys that cite it.
pub struct Missing {
    pub name: String,
    pub sections: Vec<String>,
}

/// The refs a body cites, in first-seen order: `H12`, `H4-H8` expands to
/// `H4..=H8`, and `plan/label` counts as `label`. Fenced code is skipped.
pub(super) fn refs_in(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |r: String| {
        if !out.iter().any(|o| o == &r) {
            out.push(r);
        }
    };
    let mut fenced = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut i = 0usize;
        let chars: Vec<char> = line.chars().collect();
        while i < chars.len() {
            // an H-ref starts at an uppercase H followed by a digit
            if chars[i] == 'H'
                && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit())
                && (i == 0 || !is_word(chars[i - 1]))
            {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                let first: String = chars[i..j].iter().collect();
                // a dash range: `H4-H8`, `H4 - H8`, `H4–H8`, `H4..H8`
                let (k, second) = range_after(&chars, j);
                if let Some(s) = second {
                    let e = expand(&first, &s);
                    if e.len() > 1 {
                        for n in e {
                            push(n);
                        }
                        i = k;
                        continue;
                    }
                }
                push(first);
                i = j;
                continue;
            }
            // a label-like token: letters then digits, nothing else
            if chars[i].is_ascii_alphabetic() && (i == 0 || !is_word(chars[i - 1])) {
                let mut j = i;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let mut k = j;
                while k < chars.len() && chars[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j && (k == chars.len() || !is_word(chars[k])) {
                    let tok: String = chars[i..k].iter().collect();
                    let (ls, ds) = letter_digit_split(&tok);
                    if (1..=6).contains(&ls) && (1..=3).contains(&ds) && tok.len() <= 8 {
                        push(tok);
                    }
                    i = k;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn letter_digit_split(tok: &str) -> (usize, usize) {
    let ls = tok.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    let ds = tok
        .chars()
        .skip(ls)
        .take_while(|c| c.is_ascii_digit())
        .count();
    if ls + ds == tok.len() {
        (ls, ds)
    } else {
        (0, 0)
    }
}

/// After `H<num>` ending at `j`: the end index and the second `H<num>`,
/// for `-`, `–`, `—`, `..` and `to`, with an optional second `H`.
fn range_after(chars: &[char], j: usize) -> (usize, Option<String>) {
    let mut k = j;
    while k < chars.len() && chars[k].is_whitespace() {
        k += 1;
    }
    if chars.get(k) == Some(&'.') && chars.get(k + 1) == Some(&'.') {
        k += 2;
    } else if matches!(chars.get(k), Some('-' | '–' | '—')) {
        k += 1;
    } else if chars[k..].starts_with(&['t', 'o'])
        && (k + 2 == chars.len() || !chars[k + 2].is_ascii_alphabetic())
    {
        k += 2;
    } else {
        return (j, None);
    }
    while k < chars.len() && chars[k].is_whitespace() {
        k += 1;
    }
    if chars.get(k) == Some(&'H') && chars.get(k + 1).is_some_and(|c| c.is_ascii_digit()) {
        k += 1;
    }
    let mut m = k;
    while m < chars.len() && chars[m].is_ascii_digit() {
        m += 1;
    }
    if m > k && (m == chars.len() || !is_word(chars[m])) {
        let second: String = chars[k..m].iter().collect();
        return (m, Some(format!("H{second}")));
    }
    (j, None)
}

/// `H4` to `H8` inclusive; empty when not a range (same, reversed, too wide).
fn expand(first: &str, second: &str) -> Vec<String> {
    let (a, b) = (first[1..].parse::<u32>(), second[1..].parse::<u32>());
    match (a, b) {
        (Ok(a), Ok(b)) if b > a && b - a <= 50 => (a..=b).map(|n| format!("H{n}")).collect(),
        _ => vec![first.to_string()],
    }
}

/// Refs cited by `sections` with no label in `labels`, each with the keys
/// that cite it, in first-seen order.
pub(super) fn missing(sections: &[Row], labels: &HashSet<String>) -> Vec<Missing> {
    let mut at: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for s in sections {
        for r in refs_in(&s.body) {
            if labels.contains(&r) {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(e) = at.entry(r.clone()) {
                e.insert(Vec::new());
                order.push(r.clone());
            }
            let keys = at.get_mut(&r).expect("inserted");
            if !keys.iter().any(|k| k == &s.key) {
                keys.push(s.key.clone());
            }
        }
    }
    order
        .into_iter()
        .map(|name| Missing {
            sections: at.remove(&name).unwrap_or_default(),
            name,
        })
        .collect()
}

impl Store {
    /// Labels of every chunk in `plan`, whatever its state — a cited ref is
    /// dangling only when no row carries it.
    fn labels(&self, plan: i64) -> Result<HashSet<String>, String> {
        let mut q = self
            .conn
            .prepare("SELECT label FROM chunk WHERE plan = ?1 AND label IS NOT NULL")
            .map_err(err)?;
        q.query_map([plan], |r| r.get::<_, String>(0))
            .map_err(err)?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(err)
    }

    /// Dangling refs of `plan`: section-cited `H`-refs and label-like tokens
    /// with no chunk row.
    pub fn plan_lint(&self, plan: i64) -> Result<Vec<Missing>, String> {
        let sections = super::section::list(&self.conn, plan)?;
        Ok(missing(&sections, &self.labels(plan)?))
    }
}

#[cfg(test)]
#[path = "plan_lint_tests.rs"]
mod tests;
