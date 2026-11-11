//! Usage loading for `fael stats` and `fael report`: `usage.jsonl` cut to
//! `since`, parsed in one pass, plus the log of every repo it names. Split
//! out of `usage.rs` (file-size ratchet); finding repos and reading journals
//! stays with the caller (CLI or app), core joins the loaded logs, never the
//! filesystem.

use super::usage_files;
use crate::core;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The log a stats row's repo has now — the tree + journal union the hooks
/// read, so a `store = "local"` repo (journal only) is not read as empty.
/// A repo path that no longer resolves falls back to its tree.
pub(crate) fn repo_log(repo: &str) -> core::Log {
    crate::repo_at(Path::new(repo))
        .map(|r| crate::read(&r))
        .unwrap_or_else(|_| core::fold_bumps(core::read(&Path::new(repo).join(".fael"))))
}

/// What `fael stats` and `fael report` read: `usage.jsonl` cut to `since`,
/// parsed, plus the log of every repo it names — finding repos and reading
/// journals stays with the caller (CLI or app); core joins the loaded logs,
/// never the filesystem.
pub(crate) struct Usage {
    pub(crate) path: PathBuf,
    pub(crate) empty: bool,
    pub(crate) parsed: core::stats::Parsed,
    pub(crate) logs: HashMap<String, core::Log>,
    /// The explicit `--since` ms (`Some(0)` = all, `None` = absent): the
    /// outcome window for the flow/outcome joins, else the oldest log row.
    pub(crate) since: Option<i64>,
}

pub(crate) fn load(since: Option<i64>) -> Usage {
    load_where(since, &|_| true)
}

/// `load`, keeping only the lines (and logs) of repos `keep` accepts — a repo's
/// evaluator never reads another repo's usage (SPEC-fael-learn-loop §E).
pub(crate) fn load_where(since: Option<i64>, keep: &dyn Fn(&str) -> bool) -> Usage {
    load_text(&usage_files::read(since), since, keep)
}

/// Usage text → `Parsed`, touching no repo log, git or gh.
pub(crate) fn parse_text(s: &str) -> core::stats::Parsed {
    parse_since_text(s, None)
}

/// `parse_text` with the window cut in the same pass — one parse per line
/// (`parse_since` in core; `since`'s second parse of every line is gone).
fn parse_since_text(s: &str, window: Option<i64>) -> core::stats::Parsed {
    // benchmark/test repos live in the OS temp dir (01M3CRR6A) — the boundary
    // rides in as a parameter, so core stays pure. `/tmp` too: on macOS the
    // OS temp dir is under /var/folders, but agent scratchpads use /tmp
    let tmp = [
        std::env::temp_dir(),
        std::env::temp_dir().canonicalize().unwrap_or_default(),
        PathBuf::from("/tmp"),
        Path::new("/tmp").canonicalize().unwrap_or_default(),
    ];
    core::stats::parse_since(s, &usage_files::live(), &tmp, window)
}

/// `load_where` over usage text the caller already read.
pub(crate) fn load_text(s: &str, since: Option<i64>, keep: &dyn Fn(&str) -> bool) -> Usage {
    let path = usage_files::live();
    // `--since all` (0) cuts nothing; any other window cuts in the parse pass
    let window = since.filter(|&ms| ms > 0);
    let mut parsed = parse_since_text(s, window);
    // blank text reads empty; a window that cut every line does too (torn
    // lines alone never did — they are text, so the text path still says how
    // many temp rows were skipped)
    let empty = s.trim().is_empty()
        || (window.is_some()
            && parsed.kept.is_empty()
            && parsed.calls.is_empty()
            && parsed.skipped == 0);
    parsed.kept.retain(|v| v["repo"].as_str().is_some_and(keep));
    parsed
        .calls
        .retain(|v| v["repo"].as_str().is_some_and(keep));
    let mut logs: HashMap<String, core::Log> = HashMap::new();
    // a removed worktree's path no longer leads to its journal: leave it out,
    // so stats read it as unknown, not as a log with no rows (01M4D7N4)
    for repo in parsed.repos().into_iter().filter(|r| keep(r)) {
        if Path::new(repo).exists() {
            logs.entry(repo.to_string())
                .or_insert_with(|| repo_log(repo));
        }
    }
    Usage {
        path,
        empty,
        parsed,
        logs,
        since,
    }
}
