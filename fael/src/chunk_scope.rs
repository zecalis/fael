//! `chunk add|edit --scope`: each path checked against the repo once written
//! (issue 01M4Q4HHX) — a typo'd scope silently weakens every overlap check, so it is said;
//! never refused, a chunk may create the file.

use fael_core::plan::{Store, overlap};
use std::path::Path;

/// A warning on stderr for each scope path of chunk `key` that names nothing under `root`:
/// its directory is missing, or holds a name within edit distance 2. A new file or directory under an existing one passes silently.
/// Then each path the stored brief names that exists under `root` but outside the scope.
pub(crate) fn warn(s: &Store, root: &Path, key: &str) {
    let Ok(b) = s.show(key) else { return };
    brief_paths(s, root, &b);
    for p in b.scope.as_deref().unwrap_or_default().lines() {
        if let Some(why) = missing(root, p) {
            eprintln!(
                "warning: scope {p:?} {why} in {} — a typo? fael chunk edit {key} --scope …",
                root.display()
            );
        }
    }
}

fn missing(at: &Path, p: &str) -> Option<String> {
    let p = p.trim_end_matches('/');
    if p.is_empty() || at.join(p).exists() {
        return None;
    }
    let dir = p.rsplit_once('/').map_or("", |(d, _)| d);
    if !at.join(dir).is_dir() {
        return Some(format!("names nothing: no directory {dir:?}"));
    }
    crate::write::sibling_suggest(at, p).map(|near| format!("is not on disk — {near:?}?"))
}

/// The brief is the planner's text, never the rendered start. A warning only: the scope is
/// the planner's to change, and a path not on disk may be a file the chunk creates.
/// A path on a line that says "leave alone" never warns: the planner saw it and
/// deliberately scoped it out (a `drop?` brief naming what not to touch).
fn brief_paths(s: &Store, root: &Path, b: &fael_core::plan::Brief) {
    let scope: Vec<String> = b
        .scope
        .as_deref()
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let (at, stale) = crate::board::clock();
    let others = s
        .board(&at[..10], &stale)
        .map(|b| b.chunks)
        .unwrap_or_default();
    for line in b.brief.lines() {
        if line.to_lowercase().contains("leave alone") {
            continue;
        }
        for p in paths(line) {
            let hit = [p.clone()];
            if !root.join(&p).exists() || overlap(&hit, &scope) {
                continue;
            }
            let holder = others.iter().find(|c| {
                c.uid != b.uid
                    && !["done", "replaced", "dropped"].contains(&c.state.as_str())
                    && overlap(&hit, &c.scope)
            });
            let by = holder.map_or(String::new(), |c| {
                format!(
                    " (in {}/{}'s scope)",
                    c.plan,
                    c.label.as_deref().unwrap_or(&c.uid)
                )
            });
            eprintln!(
                "warning: brief names {p}, not in scope{by} — fael chunk edit {} --scope …",
                b.uid
            );
        }
    }
}

/// Path-like tokens of `text` (a `/` and a dot, or a trailing `/`), once each, with the
/// backticks, quotes, trailing punctuation and `:line` stripped.
fn paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in text.split_whitespace() {
        let end = |c: char| "`\"'()[]<>,;.:!?".contains(c);
        let mut t = t
            .trim_start_matches(|c| "`\"'([<".contains(c))
            .trim_end_matches(end);
        while let Some((head, line)) = t.rsplit_once(':') {
            if !line.chars().all(|c| c.is_ascii_digit()) {
                break;
            }
            t = head;
        }
        let t = t.trim_end_matches(end);
        let ok = t.contains('/')
            && (t.ends_with('/') || t.contains('.'))
            && !t.starts_with('/')
            && !t.contains("://")
            && !t.contains("..")
            && !t.contains('*');
        if ok && !out.iter().any(|o| o == t) {
            out.push(t.to_string());
        }
    }
    out
}
