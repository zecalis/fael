//! Ask accounting (PLAN-fael-durable-log chunk 3a): every time fael costs
//! the agent another round — a `rejected:` write, a stop-hook block, a warning
//! line — lands in usage.jsonl with its ask type, so `fael stats` shows
//! whether self-heal (chunk 3b–e) actually asks less. The read half (transcript
//! tokens, metrics) lives in `askstats`; estimates stay labelled `est`, never
//! `token`.

use super::askstats::{RealTokens, transcript_usage};
use super::state::{now_rfc3339, state_dir};
use crate::core;
use std::path::Path;

pub(crate) const ASK_REJECT: &str = "reject";
pub(crate) const ASK_BLOCK: &str = "stop-block";
pub(crate) const ASK_WARN: &str = "warning";

/// Optional half of a usage row: what kind of ask this was (absent on plain
/// pushes), the session it belongs to (absent outside hooks), and the real
/// tokens of the round that just ended (absent without a transcript `usage`).
#[derive(Default)]
pub(crate) struct UsageMeta<'a> {
    pub ask: Option<&'a str>,
    pub session: Option<&'a str>,
    pub real: Option<RealTokens>,
}

/// Append one usage row, fail-open like the rest of this module: accounting
/// must never fail the command it rode along with.
pub(crate) fn append_row(row: serde_json::Value) {
    let path = state_dir().join("usage.jsonl");
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        use std::io::Write;
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| writeln!(f, "{row}"));
    }
}

/// The usage meta for a hook event: the ask type, the session, and the
/// transcript's latest `usage` — unless `real` is false (session-start: no
/// round completed yet, so the transcript's past tells nothing).
pub(crate) fn hook_meta<'a>(session: &'a str, ask: Option<&'a str>, real: bool) -> UsageMeta<'a> {
    UsageMeta {
        ask,
        session: session_meta(session),
        real: real.then(|| transcript_usage(session)).flatten(),
    }
}

/// This hook event's session for usage rows — absent outside hooks, so
/// session-less rows never join a per-session metric.
fn session_meta(session: &str) -> Option<&str> {
    (!session.is_empty()).then_some(session)
}

/// `add/close/bump/mv/find` in the shell: one choke point for rejects — only
/// `rejected:` counts (anything else is fael's own failure, not a round the
/// agent owes), under the command's own event so rejects join its warnings.
pub(crate) fn record_cli_reject(cmd: &str, e: &str) {
    if !e.starts_with("rejected:") {
        return;
    }
    let repo = std::env::current_dir()
        .ok()
        .and_then(|d| crate::repo_at(&d).ok());
    record_ask(
        "cli",
        ASK_REJECT,
        cmd,
        repo.as_ref().map(|r| r.root.as_path()),
        e,
    );
}

/// Every warning line the agent reads is an ask — the caller already printed
/// them; this counts them next to the rejects.
pub(crate) fn record_asks(
    client: &str,
    ask: &str,
    event: &str,
    repo: Option<&Path>,
    texts: &[String],
) {
    for t in texts {
        record_ask(client, ask, event, repo, t);
    }
}

/// One MCP tool result: a reject (always `rejected:`-prefixed — anything else
/// is fael's own failure) or a warning line the tool sends back.
pub(crate) fn record_mcp(root: &Path, tool: &str, ask: &str, text: &str) {
    if ask != ASK_WARN && !text.starts_with("rejected:") {
        return;
    }
    record_ask("mcp", ask, tool, Some(root), text);
}

/// One ask the agent has to answer: a reject, a stop-hook block (recorded by
/// the hook itself through `UsageMeta`), or a warning line. `repo` is `None`
/// when the call never resolved one (parse errors before routing) — those rows
/// still count, they just skip the temp-dir filter and the per-repo joins.
pub(crate) fn record_ask(client: &str, ask: &str, event: &str, repo: Option<&Path>, text: &str) {
    append_row(serde_json::json!({
        "ts": now_rfc3339().unwrap_or_default(),
        "repo": repo.map(|r| r.to_string_lossy().into_owned()).unwrap_or_default(),
        "client": client,
        "event": event,
        "ask": ask,
        "bytes": text.len(),
        "est_tokens": core::est_tokens(text),
    }));
}

/// Bytes the agent pays every session before saying anything: the bundled
/// SKILL.md plus the MCP tool schemas — both local, no fetch. The chunk-6
/// ceiling test pins these; stats shows them so the cut is verifiable.
pub(crate) fn constants() -> (usize, usize, usize, usize) {
    const SKILL: &str = include_str!("../../skill/SKILL.md");
    let schema = crate::mcp::schema_json();
    (
        SKILL.len(),
        core::est_tokens(SKILL),
        schema.len(),
        core::est_tokens(&schema),
    )
}
