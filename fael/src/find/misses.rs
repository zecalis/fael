//! A text find that matched nothing: say why, and keep the query so a vector
//! index (decision 01M3ST4V — "until a recorded miss") has data to be judged
//! by. Local only: `find-misses.jsonl` sits beside `usage.jsonl`, never in git.
//! Fails open — recording must never fail the find it rode along with.

use crate::core::{self, Filter, Log};
use crate::hook::{now_rfc3339, state_dir};
use std::path::{Path, PathBuf};

fn path() -> PathBuf {
    state_dir().join("find-misses.jsonl")
}

/// What an empty find prints, and (for a text search) records. A miss on a
/// file or key alone is just "nothing filed there" — only words can be a
/// vocabulary miss. `files_flag` is how the caller spells `--files`.
pub(crate) fn explain(
    repo: &Path,
    client: &str,
    log: &Log,
    f: &Filter,
    files_flag: &str,
) -> String {
    let why = core::why_empty(log, f, files_flag);
    if let Some(q) = f.text.as_deref().filter(|t| !t.trim().is_empty()) {
        let row = serde_json::json!({
            "ts": now_rfc3339().unwrap_or_default(),
            "repo": repo.to_string_lossy(),
            "client": client,
            "text": q,
            "why": why,
        });
        append(&row);
    }
    why
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

/// The line `fael stats` adds when text searches came back empty — absent at
/// zero, so a repo with no misses prints what it always did.
pub(crate) fn stats_line() -> Option<String> {
    let n = read().len();
    (n > 0).then(|| {
        format!("  find misses: ×{n} empty text searches (fael stats --misses lists them)")
    })
}

/// `fael stats --misses`: the newest empty text searches, one per line, with
/// the per-word counts that said which word the rows never use.
pub(crate) fn print_recent(limit: usize) {
    let all = read();
    if all.is_empty() {
        println!("fael: no empty text searches recorded");
        return;
    }
    for m in all.iter().rev().take(limit) {
        let g = |k: &str| m[k].as_str().unwrap_or("");
        println!(
            "- {} {:?} — {}",
            g("ts").get(..10).unwrap_or(g("ts")),
            g("text"),
            g("why")
                .trim_start_matches("no rows match — each alone: ")
                .trim_start_matches("no rows match ")
        );
    }
}
