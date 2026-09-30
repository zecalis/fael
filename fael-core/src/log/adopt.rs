//! `fael migrate local`: fold the tree log (`.fael/log`) into the clone's
//! journal before the tree goes. The tree copy wins on every id it holds —
//! a row edited in the tree (a PR that renamed its files) would otherwise fall
//! back to the journal's stale original once `.fael/log` is removed, since
//! the tree no longer wins on duplicate ids. Rows only the tree holds (other
//! writers, `_import/`, rows from before the journal) are copied over.
//! Idempotent: a second run changes nothing.

use super::{collect_files, decode_text, is_marker, lock, tmp_rename};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What one fold did: lines copied from the tree, stale journal lines it
/// replaced or dropped, and the journal files rewritten.
#[derive(Debug, Default)]
pub struct Adopted {
    pub copied: usize,
    pub replaced: usize,
    pub files: Vec<PathBuf>,
}

/// A row's id; `None` for a blank, marker or unparsable line (kept as is).
fn line_id(line: &str) -> Option<String> {
    if line.trim().is_empty() || is_marker(line) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    v.get("id")?.as_str().map(String::from)
}

/// Complete lines only — a torn last line (no `\n`) is skipped, as on read.
fn lines(p: &Path) -> Result<Vec<String>, String> {
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut v: Vec<String> = decode_text(&b)
        .split('\n')
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();
    v.pop();
    Ok(v)
}

/// `.jsonl` files under `root`, as `(path relative to root, lines)`.
fn files(root: &Path) -> Result<Vec<(PathBuf, Vec<String>)>, String> {
    let mut out = vec![];
    for f in collect_files(root) {
        if !f.to_string_lossy().ends_with(".jsonl") {
            continue;
        }
        let rel = f.strip_prefix(root).unwrap_or(&f).to_path_buf();
        out.push((rel, lines(&f)?));
    }
    Ok(out)
}

/// Rows and closes are separate id spaces (format.md §Readers).
fn stream(rel: &Path) -> bool {
    rel.to_string_lossy().ends_with(".close.jsonl")
}

pub fn adopt_tree(fael: &Path, journal: &Path) -> Result<Adopted, String> {
    let _guards = [lock(fael)?, lock(journal)?];
    let (troot, jroot) = (fael.join("log"), journal.join("log"));
    let tree = files(&troot)?;
    // every tree line per (stream, id): the only versions the journal may keep
    let mut wins: HashMap<(bool, String), HashSet<&str>> = HashMap::new();
    for (rel, ls) in &tree {
        for l in ls {
            if let Some(id) = line_id(l) {
                wins.entry((stream(rel), id)).or_default().insert(l);
            }
        }
    }
    let by_rel: HashMap<&Path, &Vec<String>> = tree.iter().map(|(r, l)| (r.as_path(), l)).collect();
    let mut out = Adopted::default();
    let journal_files = files(&jroot)?;
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for (rel, before) in &journal_files {
        seen.insert(rel.clone());
        let tl = by_rel.get(rel.as_path()).map_or(&[][..], |v| v.as_slice());
        let after = fold(before, tl, stream(rel), &wins, &mut out);
        if &after != before {
            write(&jroot.join(rel), &after, &mut out)?;
        }
    }
    for (rel, tl) in &tree {
        if !seen.contains(rel) && !tl.is_empty() {
            let after = fold(&[], tl, stream(rel), &wins, &mut out);
            write(&jroot.join(rel), &after, &mut out)?;
        }
    }
    Ok(out)
}

/// One journal file after the fold: a stale version is swapped for the tree
/// copy in place (so the file keeps its order), a stale version whose tree
/// copy sits in another file is dropped, and tree lines still missing are
/// appended.
fn fold(
    before: &[String],
    tl: &[String],
    close: bool,
    wins: &HashMap<(bool, String), HashSet<&str>>,
    out: &mut Adopted,
) -> Vec<String> {
    let mut after: Vec<String> = vec![];
    let mut have: HashSet<&str> = HashSet::new();
    let mut copied = 0;
    for l in before {
        let id = line_id(l);
        let w = id.as_ref().and_then(|id| wins.get(&(close, id.clone())));
        match w {
            // not a row, or an id the tree never held: the journal's own
            None => after.push(l.clone()),
            Some(w) if w.contains(l.as_str()) => {
                if have.insert(l) {
                    after.push(l.clone());
                }
            }
            Some(_) => {
                out.replaced += 1;
                for t in tl.iter().filter(|t| line_id(t) == id) {
                    if have.insert(t) {
                        copied += 1;
                        after.push(t.clone());
                    }
                }
            }
        }
    }
    for t in tl.iter().filter(|t| line_id(t).is_some()) {
        if have.insert(t) {
            copied += 1;
            after.push(t.clone());
        }
    }
    out.copied += copied;
    after
}

fn write(path: &Path, ls: &[String], out: &mut Adopted) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let mut body = ls.join("\n");
    if !ls.is_empty() {
        body.push('\n');
    }
    tmp_rename(path, body.as_bytes())?;
    out.files.push(path.to_path_buf());
    Ok(())
}
