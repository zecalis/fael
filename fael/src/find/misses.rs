//! A text find that matched nothing: say why, and keep the query so a vector
//! index (decision 01M3ST4V — "until a recorded miss") has data to be judged
//! by. Local only: `find-misses.jsonl` sits beside `usage.jsonl`, never in git.
//! Fails open — recording must never fail the find it rode along with.
//!
//! A miss on words is a vocabulary miss (`text`); a miss on a file or key
//! alone is just "nothing filed there" — it rides its own `files`/`key`
//! fields, never the `text` vocabulary list, so the word data stays clean.

use crate::core::{self, Filter, Log, Row};
use crate::hook::{now_rfc3339, state_dir};
use std::path::{Path, PathBuf};

fn path() -> PathBuf {
    state_dir().join("find-misses.jsonl")
}

/// What an empty find prints instead: the nearest rows (`core::nearest`)
/// under their label, else why nothing matched (no rows). A text search is
/// recorded either way — the words still missed. An empty `--files`/`--key`
/// find is recorded too, on its own `files`/`key` fields — never as `text`,
/// so file/key misses stay out of the vocabulary list. Only
/// an answer with no rows counts as empty for friction. `files_flag` is how
/// the caller spells `--files`.
pub(crate) fn explain<'a>(
    repo: &Path,
    client: &str,
    log: &'a Log,
    f: &Filter,
    files_flag: &str,
) -> (String, Vec<&'a Row>) {
    match core::nearest(log, f) {
        Some(near) => near,
        None => {
            // still nothing, even closed: the miss is real, so record it — a
            // fallback answer above is no miss and stays out of the log
            let why = core::why_empty(log, f, files_flag);
            record_miss(repo, client, f, &why);
            crate::hook::note_empty();
            (why, vec![])
        }
    }
}

/// Keep the query that matched nothing: words under `text`, files under
/// `files`, the key under `key` — each omitted when absent so old readers
/// (which read `text`/`why` only) keep working. A call with no text, files
/// or key constrains nothing, so there is no miss to keep.
fn record_miss(repo: &Path, client: &str, f: &Filter, why: &str) {
    let text = f.text.as_deref().filter(|t| !t.trim().is_empty());
    let has_files = !f.files.is_empty();
    let key = f.key.as_deref().filter(|k| !k.is_empty());
    if text.is_none() && !has_files && key.is_none() {
        return;
    }
    let mut row = serde_json::json!({
        "ts": now_rfc3339().unwrap_or_default(),
        "repo": repo.to_string_lossy(),
        "client": client,
        "why": why,
    });
    if let Some(q) = text {
        row["text"] = q.into();
    }
    if has_files {
        row["files"] = f.files.clone().into();
    }
    if let Some(k) = key {
        row["key"] = k.into();
    }
    append(&row);
}

fn append(row: &serde_json::Value) {
    use std::io::Write;
    let p = path();
    if let Some(dir) = p.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        // one write per row, like usage.jsonl: O_APPEND keeps rows whole
        let line = format!("{row}\n");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .and_then(|mut f| f.write_all(line.as_bytes()));
    }
}

fn read() -> Vec<serde_json::Value> {
    std::fs::read_to_string(path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The line `fael stats` adds when finds came back empty — absent at
/// zero, so a repo with no misses prints what it always did.
pub(crate) fn stats_line() -> Option<String> {
    let n = read().len();
    (n > 0).then(|| {
        format!("  find misses: ×{n} finds matched no rows (fael stats --misses lists them)")
    })
}

/// `fael stats --misses`: the newest finds that matched no rows, one per line, with
/// the per-word counts that said which word the rows never use. Words print
/// quoted under `text`; a files/key-only miss prints its own fields, never as
/// words, so the vocabulary list stays words-only.
pub(crate) fn print_recent(limit: usize) {
    let all = read();
    if all.is_empty() {
        println!("fael: no finds that matched no rows recorded");
        return;
    }
    for m in all.iter().rev().take(limit) {
        let g = |k: &str| m[k].as_str().unwrap_or("");
        let mut q = String::new();
        if !g("text").is_empty() {
            q.push_str(&format!("{:?}", g("text")));
        }
        if let Some(files) = m.get("files").and_then(|v| v.as_array()) {
            let list = files
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(",");
            if !q.is_empty() {
                q.push(' ');
            }
            q.push_str(&format!("files:{list}"));
        }
        if let Some(k) = m
            .get("key")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            if !q.is_empty() {
                q.push(' ');
            }
            q.push_str(&format!("key:{k}"));
        }
        if q.is_empty() {
            q.push_str("\"\"");
        }
        println!(
            "- {} {q} — {}",
            g("ts").get(..10).unwrap_or(g("ts")),
            g("why")
                .trim_start_matches("no rows match — each alone: ")
                .trim_start_matches("no rows match ")
                .trim_start_matches("no row mentions ")
        );
    }
}
