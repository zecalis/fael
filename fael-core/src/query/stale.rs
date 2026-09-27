//! `[Stale]` — row text that still names a path which is gone (row-hygiene
//! chunk 4). `files[]` rot is already covered by `[Gone]`/`[PartGone]`; this
//! is about the prose: a backticked `` `path` `` that has no file behind it
//! any more, so the next reader follows a dead pointer.
//!
//! Only backticked spans count (a bare word is talk, a backtick is a pointer),
//! and only spans that look like paths: they hold a `/`, or end in a file
//! extension whose letters prove it is not a version number (`0.45` is not a
//! path, `pnpm-workspace.yaml` is). Existence is judged like `gone_files` —
//! through the alias resolver, so a rename still resolves and never flags.

use crate::Aliases;
use crate::Row;
use std::path::Path;

/// Backticked spans of `text` that look like paths (`/` inside, or a trailing
/// `name.ext`). Multiline spans (fenced code blocks) never count — those are
/// commands and output, not pointers.
pub fn backtick_paths(text: &str) -> Vec<&str> {
    let mut out = vec![];
    for (i, span) in text.split('`').enumerate() {
        if i % 2 == 0 {
            continue; // outside backticks
        }
        let s = span.trim();
        if s.is_empty() || s.contains('\n') {
            continue;
        }
        if s.contains('/') || has_extension(s) {
            out.push(s);
        }
    }
    out
}

/// The row's backticked paths that resolve nowhere under `root`, even through
/// `al`. Paths the row already files (exact match) stay `[PartGone]`'s job.
pub fn stale_refs(root: &Path, row: &Row, al: &Aliases) -> Vec<String> {
    let mut out = vec![];
    for c in backtick_paths(&row.text) {
        if row.files.iter().any(|f| f == c) {
            continue;
        }
        let p = c.strip_prefix('/').unwrap_or(c);
        let p = p.strip_prefix("./").unwrap_or(p);
        if al.forward(p).iter().all(|q| !root.join(q).exists()) && !out.contains(&p.to_string()) {
            out.push(p.to_string());
        }
    }
    out
}

/// `name.ext` where both sides look like a file, not a version: the name
/// holds a letter (`1.x` is a version, `file2.rs` is a path), the tail after
/// the last `.` is short and holds a letter too (`0.45` is a version).
/// A leading-dot base (`.gitignore`) is a path whatever follows the dot.
fn has_extension(s: &str) -> bool {
    let base = s.rsplit('/').next().unwrap_or(s);
    if base.starts_with('.') {
        return base.len() > 1;
    }
    let Some((name, ext)) = base.rsplit_once('.') else {
        return false;
    };
    !name.is_empty()
        && name.chars().any(|c| c.is_ascii_alphabetic())
        && !ext.is_empty()
        && ext.len() <= 5
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic())
}
