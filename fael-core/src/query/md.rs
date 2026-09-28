//! `[Phantom]` in markdown (issue `ids:doctor-plans`): every `*.md` under the
//! repo is prose the next agent follows, so an id citation with no row behind
//! it is as dead there as one in a row's text. A ULID inside a fenced code
//! block is an example, never a citation (`docs/format.md` ships one), so
//! fences are skipped the way `backtick_paths` skips multiline spans.
//!
//! Scope is a plain walk of `root`, not `cfg.plan_dirs`: that field was dropped
//! with the plan resolver (#66), and plan files are gitignored here (`.fapony/`),
//! so `git ls-files` would miss exactly the files that matter.

use super::matching::is_md;
use super::refs::phantom_refs;
use crate::Log;
use std::path::{Path, PathBuf};

/// `(repo-relative path, 1-based line, token)` for every id-shaped token in
/// markdown prose with no row behind it — union scope, so a real or ambiguous
/// prefix resolves and never reports. One entry per file and token (the first
/// line it appears on). Pure: no git spawn, so the binary re-checks the
/// tokens against unmerged branches once, exactly as it does for rows.
pub fn phantom_md_refs(log: &Log, root: &Path) -> Vec<(String, usize, String)> {
    let mut out: Vec<(String, usize, String)> = vec![];
    for path in md_files(root) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = crate::log::decode_text(&bytes);
        let rel = rel(&path, root);
        let mut fence: Option<(u8, usize)> = None;
        for (n, line) in text.lines().enumerate() {
            if let Some((c, open)) = fence {
                if closes_fence(line, c, open) {
                    fence = None;
                }
                continue;
            }
            if let Some(f) = opens_fence(line) {
                fence = Some(f);
                continue;
            }
            for tok in phantom_refs(log, line) {
                if !out.iter().any(|(p, _, t)| p == &rel && *t == tok) {
                    out.push((rel.clone(), n + 1, tok));
                }
            }
        }
    }
    out
}

/// Every `*.md` under `root`, sorted — the walk `multi_fael` takes, skipping
/// `.git`/`target`/`node_modules`. `DirEntry::file_type` never follows a
/// symlink, so a symlinked directory is not descended into (a loop would hang
/// `doctor`).
fn md_files(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    walk(root, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else {
            continue;
        };
        let p = e.path();
        if ft.is_dir() {
            if [".git", "target", "node_modules"]
                .contains(&e.file_name().to_string_lossy().as_ref())
            {
                continue;
            }
            walk(&p, out);
        } else if is_md(&p.to_string_lossy()) {
            out.push(p);
        }
    }
}

/// The trimmed line opens a fence: ≥3 `` ` `` or `~` at its start.
fn opens_fence(line: &str) -> Option<(u8, usize)> {
    let t = line.trim_start().as_bytes();
    for c in *b"`~" {
        let open = t.iter().take_while(|&&b| b == c).count();
        if open >= 3 {
            return Some((c, open));
        }
    }
    None
}

/// The trimmed line closes it: the same byte run, at least as long, nothing
/// but whitespace after — an info string (` ```rust `) never closes a fence.
fn closes_fence(line: &str, c: u8, open: usize) -> bool {
    let t = line.trim_start().as_bytes();
    let run = t.iter().take_while(|&&b| b == c).count();
    run >= open && t[run..].iter().all(u8::is_ascii_whitespace)
}

/// The path as the reader writes it: relative to the repo root, `/` separated.
fn rel(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
