//! Dual-env session/client pick — one rule read at many call sites.
//! `FAEL_SESSION` first; Claude+Codex both set picks the id whose hook record
//! in this worktree is newest, else today's order. MCP client from initialize.

use crate::hook;
use std::path::{Path, PathBuf};

static MCP_CLIENT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
pub(crate) fn mark_mcp_client(name: Option<&str>) {
    let _ = MCP_CLIENT.set(name.and_then(map_mcp_client).map(str::to_string));
}
pub(crate) fn mcp_client() -> Option<&'static str> {
    MCP_CLIENT.get().and_then(|o| o.as_deref())
}
pub(crate) fn map_mcp_client(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    if n.contains("claude") {
        Some("claude")
    } else if n.contains("codex") {
        Some("codex")
    } else if n.contains("opencode") {
        Some("opencode")
    } else {
        None
    }
}

fn record_time(root: &Path, id: &str) -> Option<std::time::SystemTime> {
    let m = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    [
        m(hook::edits_path(id, root)),
        m(hook::started_path(id, root)),
    ]
    .into_iter()
    .flatten()
    .max()
}

pub(crate) fn pick(root: Option<&Path>) -> (Option<&'static str>, String) {
    let fael = std::env::var("FAEL_SESSION").unwrap_or_default();
    let claude = std::env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default();
    let codex = std::env::var("CODEX_THREAD_ID").unwrap_or_default();
    if !fael.is_empty() {
        return (Some("opencode"), fael);
    }
    if !claude.is_empty()
        && !codex.is_empty()
        && let Some(r) = root
    {
        match (record_time(r, &claude), record_time(r, &codex)) {
            (Some(a), Some(b)) if a != b => {
                return if b > a {
                    (Some("codex"), codex)
                } else {
                    (Some("claude"), claude)
                };
            }
            (None, Some(_)) => return (Some("codex"), codex),
            (Some(_), None) => return (Some("claude"), claude),
            _ => {}
        }
    }
    if !claude.is_empty() {
        return (Some("claude"), claude);
    }
    if !codex.is_empty() {
        return (Some("codex"), codex);
    }
    (None, String::new())
}

/// Writer client for a row: MCP initialize name over MCP, else CLI env client
/// when it names an agent. `cli`/unknown stamps nothing (`writer_unknown`).
pub(crate) fn writer_client(root: &Path) -> Option<String> {
    if crate::session::is_mcp() {
        return mcp_client().map(str::to_string);
    }
    match pick(Some(root)).0 {
        None | Some("cli") => None,
        Some(c) => Some(c.to_string()),
    }
}
