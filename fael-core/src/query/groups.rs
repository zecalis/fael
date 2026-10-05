//! `find --groups`: open rows that share a file, one group each — what to fix
//! together in one PR. Union-find over the rows' files; no guessing beyond
//! "the same path".

use super::matching::is_md;
use crate::{Log, Row};
use std::collections::HashMap;

/// Partition `rows` into groups linked by a shared file, largest first, rows
/// in their given (ranked) order inside a group; a row sharing nothing is a
/// group of one, after the rest. Anchors (`doc:x`, `plan:y`) and `*.md` never
/// link: a spec cited by every issue is context, not a shared edit.
// ponytail: a hub code file (lib.rs, mod.rs) still links everything that cites it;
// a skip list when that shows up in real groups.
pub fn groups<'a>(rows: &[&'a Row]) -> Vec<Vec<&'a Row>> {
    let mut parent: Vec<usize> = (0..rows.len()).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut owner: HashMap<&str, usize> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        for f in r.files.iter().filter(|f| links(f)) {
            match owner.get(f.as_str()) {
                Some(&j) => {
                    let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                    parent[a.max(b)] = a.min(b);
                }
                None => {
                    owner.insert(f, i);
                }
            }
        }
    }
    let mut by_root: Vec<Vec<&Row>> = vec![vec![]; rows.len()];
    for (i, r) in rows.iter().enumerate() {
        by_root[root(&mut parent, i)].push(r);
    }
    let mut out: Vec<Vec<&Row>> = by_root.into_iter().filter(|g| !g.is_empty()).collect();
    // stable: equal sizes keep the rank of their first row
    out.sort_by_key(|g| std::cmp::Reverse(g.len()));
    out
}

fn links(f: &str) -> bool {
    crate::anchor(f).is_none() && !is_md(f)
}

/// The files a group's rows share (cited by two or more of them), sorted.
fn shared<'a>(g: &[&'a Row]) -> Vec<&'a str> {
    let mut n: HashMap<&str, usize> = HashMap::new();
    for f in g.iter().flat_map(|r| r.files.iter()).filter(|f| links(f)) {
        *n.entry(f).or_default() += 1;
    }
    let mut s: Vec<&str> = n
        .into_iter()
        .filter(|(_, c)| *c > 1)
        .map(|(f, _)| f)
        .collect();
    s.sort_unstable();
    s
}

/// Every group under one header line, each row as `render` prints it. No
/// token budget: a group cut in half would answer the question wrong.
pub fn render_groups(log: &Log, rows: &[&Row]) -> String {
    let mut out = String::new();
    for (i, g) in groups(rows).iter().enumerate() {
        if g.len() == 1 {
            out.push_str(&format!("## group {} · shares no file\n", i + 1));
        } else {
            out.push_str(&format!(
                "## group {} · {} rows · shared: {}\n",
                i + 1,
                g.len(),
                shared(g).join(", ")
            ));
        }
        out.push_str(&super::render(log, g, usize::MAX));
    }
    // render ends each group with the same `bodies:` hint — say it once, last
    let hint = out
        .lines()
        .find(|l| l.starts_with("bodies: "))
        .map(str::to_owned);
    let mut out: String = out
        .lines()
        .filter(|l| !l.starts_with("bodies: "))
        .map(|l| format!("{l}\n"))
        .collect();
    if let Some(h) = hint {
        out.push_str(&format!("{h}\n"));
    }
    out
}
