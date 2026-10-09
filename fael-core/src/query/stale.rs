//! `[Stale]` — row text that still names a path which is gone (row-hygiene
//! chunk 4). `files[]` rot is already covered by `[Gone]`/`[PartGone]`; this
//! is about the prose: a backticked `` `path` `` that has no file behind it
//! any more, so the next reader follows a dead pointer.
//!
//! Only backticked spans count (a bare word is talk, a backtick is a pointer),
//! and only spans that look like paths: they hold a `/`, or end in a file
//! extension whose letters prove it is not a version number (`0.45` is not a
//! path, `pnpm-workspace.yaml` is). A `file.rs:88` citation is judged by the
//! path alone, and a URL (`://`) is never a repo path. Existence is judged like
//! `gone_files` — through the alias resolver, so a rename still resolves and
//! never flags.

use crate::Aliases;
use crate::Log;
use crate::Row;
use std::path::Path;

/// Backticked spans of `text` that look like paths (`/` inside, or a trailing
/// `name.ext`). Multiline spans (fenced code blocks) and URLs never count —
/// those are commands, output and links, not repo pointers.
pub fn backtick_paths(text: &str) -> Vec<&str> {
    let mut out = vec![];
    for (i, span) in text.split('`').enumerate() {
        if i % 2 == 0 {
            continue; // outside backticks
        }
        let s = span.trim();
        if s.is_empty() || s.contains('\n') || s.contains("://") {
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
    gone_refs(root, &row.text, &row.files, al)
}

/// The backticked paths in the text `row` was closed with (its close record,
/// not the row — PLAN-fael-experience-loop chunk 3) that resolve nowhere: a
/// check the agent pointed the close at, gone since. The newest close wins.
/// Only a span holding a `/` is a check: a close's cause → fix prose names
/// bare files (`a.rs`) that live under some dir, never at the root.
pub fn stale_close_refs(root: &Path, log: &Log, row: &Row, al: &Aliases) -> Vec<String> {
    let close = log
        .closes
        .iter()
        .filter(|c| c.reference.as_deref() == Some(row.id.as_str()))
        .max_by(|a, b| a.ts.cmp(&b.ts));
    let mut gone = close.map_or(vec![], |c| gone_refs(root, &c.text, &row.files, al));
    gone.retain(|p| p.contains('/'));
    gone
}

/// The text `row` was closed with when it names its fix — a sha or `(#N)`,
/// what `fael stats` counts as fixed (PLAN-fael-experience-loop chunk 6):
/// the newest close record, else the close `fael compact` folded in. A fix
/// linked only by a commit naming the id is not here: the push reads no git.
pub fn fix_close<'a>(log: &'a Log, row: &'a Row) -> Option<&'a str> {
    let close = log
        .closes
        .iter()
        .filter(|c| c.reference.as_deref() == Some(row.id.as_str()))
        .max_by(|a, b| a.ts.cmp(&b.ts))
        .map(|c| c.text.as_str());
    let folded = || row.extra.get("closed").and_then(|c| c["text"].as_str());
    close.or_else(folded).filter(|t| crate::stats::names_fix(t))
}

fn gone_refs(root: &Path, text: &str, filed: &[String], al: &Aliases) -> Vec<String> {
    let mut out = vec![];
    for c in backtick_paths(text) {
        let p = c.strip_prefix('/').unwrap_or(c);
        let p = p.strip_prefix("./").unwrap_or(p);
        // `file.rs:88` cites a line inside a file — the file is what must exist
        let p = strip_location(p);
        // a span with whitespace is a command (`fael find --files <dir>/`),
        // never a path that must exist
        if p.is_empty() || p.contains(char::is_whitespace) || filed.iter().any(|f| f == p) {
            continue;
        }
        // a query, placeholder or glob (`dev/ui?s=x`, `e2e/<flow>.ts`,
        // `src/*.rs`) names no one file (#293)
        if p.contains(['?', '<', '>', '*']) {
            continue;
        }
        // a bare `name.ext` is a file only by a file's extension: `money.read`
        // and `document.type` are code identifiers (#293)
        if !p.contains('/') && !p.starts_with('.') && !file_ext(p) {
            continue;
        }
        // no file name, and a first dir this repo does not have: a name from
        // elsewhere (`verapdf/cli`, a docker image), never one of its paths.
        // A file (`t/gone.sh`) is still judged — its dir may be what went.
        // ponytail: a deleted top-level dir cited without a file goes unflagged
        if let Some((top, _)) = p.split_once('/')
            && !has_extension(p)
            && !root.join(top).exists()
        {
            continue;
        }
        if al.forward(p).iter().all(|q| !root.join(q).exists()) && !out.contains(&p.to_string()) {
            out.push(p.to_string());
        }
    }
    out
}

/// `path:line[:col]` cites a location inside a file — only the path before the
/// trailing run of up to two `:<digits>` groups is what must exist. A
/// non-numeric tail (a Windows drive, an anchor) is left untouched.
fn strip_location(p: &str) -> &str {
    let mut rest = p;
    for _ in 0..2 {
        let Some((head, tail)) = rest.rsplit_once(':') else {
            break;
        };
        if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
            break;
        }
        rest = head;
    }
    rest
}

/// Extensions a bare `name.ext` must end in to be judged as a file — the
/// rest (`.read`, `.type`, `.copy`) are field and method names.
/// ponytail: a fixed list; a gone file with an unlisted extension goes unsaid
const FILE_EXTS: &[&str] = &[
    "rs", "toml", "lock", "md", "mdx", "txt", "json", "jsonc", "yaml", "yml", "ts", "tsx", "js",
    "jsx", "mjs", "cjs", "py", "go", "java", "kt", "kts", "swift", "c", "h", "cc", "cpp", "hpp",
    "cs", "rb", "php", "sh", "bash", "zsh", "fish", "ps1", "sql", "html", "css", "scss", "vue",
    "svelte", "xml", "csv", "env", "ini", "cfg", "conf", "mod", "sum", "gradle", "proto",
    "graphql", "gql", "prisma", "dart", "lua", "zig", "ex", "exs", "pdf", "png", "svg",
];

/// A bare name ending in one of `FILE_EXTS`, any case (`Cargo.toml`, `A.TS`).
fn file_ext(s: &str) -> bool {
    has_extension(s)
        && s.rsplit_once('.')
            .is_some_and(|(_, e)| FILE_EXTS.iter().any(|x| x.eq_ignore_ascii_case(e)))
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
