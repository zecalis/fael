//! Reading and appending `.fael/log/**` (format.md §Layout, §Writers, §Readers).
//! Reads never fail and take no lock; appends hold `.fael/.lock` and write one whole line.
//!
//! Thin entry only — the read side stays here, the write side lives in
//! `append` (locking, add/bump/close/mv). Public paths never change —
//! `fael_core::…` and `crate::log::…` resolve as before.

mod append;

pub use append::{
    BumpOpts, MONTH_MAX, add, add_row, append, bump_row, close, close_row, mv_row, needs_seal,
};
pub(crate) use append::{lock, tmp_rename};

use crate::Row;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Every file under `dir`, sorted by path — what `read` and the maintenance
/// commands (`doctor`, `compact`, `import`) all walk.
pub(crate) fn collect_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![];
    walk(dir, &mut files);
    files.sort();
    files
}

/// A leftover merge-conflict marker line — skipped on read (both sides' rows
/// kept), stripped by `doctor --fix`.
pub(crate) fn is_marker(line: &str) -> bool {
    ["<<<<<<<", "=======", ">>>>>>>", "|||||||"]
        .iter()
        .any(|m| line.starts_with(m))
}

/// Everything under `.fael/log/`, deduped by id (first by file order wins).
#[derive(Debug, Default)]
pub struct Log {
    pub rows: Vec<Row>,
    pub closes: Vec<Row>,
    /// `file:line: what` for every line that was skipped — never fatal.
    pub warnings: Vec<String>,
}

/// Read every log file under `<fael>/log/`. Missing dir = empty log. Never errors.
pub fn read(fael: &Path) -> Log {
    let mut log = Log::default();
    for f in &collect_files(&fael.join("log")) {
        let name = f.to_string_lossy();
        if !name.ends_with(".jsonl") {
            continue;
        }
        let Ok(bytes) = fs::read(f) else {
            log.warnings.push(format!("{name}: unreadable — skipped"));
            continue;
        };
        let out = if name.ends_with(".close.jsonl") {
            &mut log.closes
        } else {
            &mut log.rows
        };
        parse(&bytes, &name, out, &mut log.warnings);
    }
    dedupe_ids(&mut log.rows);
    dedupe_ids(&mut log.closes);
    log
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out)
        } else {
            out.push(p)
        }
    }
}

/// Raw file bytes as text: invalid UTF-8 becomes U+FFFD, a BOM is stripped.
/// Shared by `parse` and `import` so both normalise the same bytes the same way.
pub fn decode_text(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    match String::from_utf8_lossy(bytes) {
        Cow::Borrowed(s) => Cow::Borrowed(s.strip_prefix('\u{feff}').unwrap_or(s)),
        Cow::Owned(s) => Cow::Owned(s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s)),
    }
}

/// Parse one file's bytes into rows. BOM, CRLF and invalid UTF-8 are normalised in memory;
/// merge-conflict markers are skipped (both sides' rows kept); a torn last line (no `\n`) is ignored.
pub fn parse(bytes: &[u8], file: &str, out: &mut Vec<Row>, warnings: &mut Vec<String>) {
    let text = decode_text(bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    let tail = lines.pop().unwrap_or("");
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if line.is_empty() || is_marker(line) {
            continue;
        }
        match serde_json::from_str::<Row>(line) {
            Ok(r) => out.push(r),
            Err(e) => warnings.push(format!("{file}:{}: broken line skipped ({e})", i + 1)),
        }
    }
    if !tail.trim().is_empty() {
        warnings.push(format!(
            "{file}:{}: torn last line (no \\n) ignored",
            lines.len() + 1
        ));
    }
}

/// Drop duplicate `id`s, first by file order wins — shared by `read`,
/// `compact` and `import` (a union merge duplicates lines everywhere).
pub(crate) fn dedupe_ids(rows: &mut Vec<Row>) {
    let mut seen = HashSet::new();
    rows.retain(|r| r.id.is_empty() || seen.insert(r.id.clone()));
}

/// `yyyy-mm`, nothing else — the one predicate behind `month_of` (file
/// stems), `append` (row timestamps) and the CLI's `--before`.
pub fn is_month(s: &str) -> bool {
    s.len() == 7
        && s.as_bytes()[4] == b'-'
        && s.bytes()
            .enumerate()
            .all(|(i, c)| i == 4 || c.is_ascii_digit())
}

/// `log/<writer>/<yyyy-mm>[.close].jsonl` → the month; anything else → None
/// (compact files and imports never match, so they are never rewritten).
pub(crate) fn month_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let stem = name
        .strip_suffix(".jsonl")?
        .strip_suffix(".close")
        .unwrap_or(name.strip_suffix(".jsonl")?);
    is_month(stem).then(|| stem.to_string())
}
