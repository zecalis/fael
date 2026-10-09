//! Ask accounting (PLAN-fael-durable-log chunk 3a): every time fael costs
//! the agent another round — a `rejected:` write or a warning
//! line — lands in usage.jsonl with its ask type, so `fael stats` shows
//! whether self-heal (chunk 3b–e) actually asks less. The read half (transcript
//! tokens, metrics) lives in `askstats`; estimates stay labelled `est`, never
//! `token`.

use super::askstats::{RealTokens, transcript_usage};
use super::protocol::Ctx;
use super::state::now_rfc3339;
use crate::core;
use std::path::Path;

pub(crate) use crate::core::stats::{ASK_REJECT, ASK_WARN};

/// Optional half of a usage row: what kind of ask this was (absent on plain
/// pushes), the session it belongs to (absent outside hooks), the sub-agent
/// whose context it landed in (absent on the session's own thread), and the
/// real tokens of the round that just ended (absent without a transcript
/// `usage`), and what the reply said per kind (`Reply::said`, absent when empty).
#[derive(Default)]
pub(crate) struct UsageMeta<'a> {
    pub ask: Option<&'a str>,
    pub session: Option<&'a str>,
    pub agent: Option<&'a str>,
    pub real: Option<RealTokens>,
    pub said: &'a [super::say::Said],
    /// The repo-relative files a push was about (absent on every other line):
    /// what lets stats see two sessions at one file, and the push's decision record.
    pub files: &'a [String],
    /// A push's decision record (`decision::record`): its keys join the line.
    pub decision: Option<&'a serde_json::Value>,
}

/// Append one usage row, fail-open like the rest of this module: accounting
/// must never fail the command it rode along with.
pub(crate) fn append_row(row: serde_json::Value) {
    super::usage_files::rotate();
    let path = super::usage_files::live();
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        use std::io::Write;
        // one write per row: concurrent writers can no longer splice bytes
        // mid-line the way writeln!'s chunked Display writes did — O_APPEND
        // lands the whole buffer at the then-current end.
        let mut line = row.to_string();
        line.push('\n');
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(line.as_bytes()));
    }
}

/// The usage meta for a hook event: the ask type, the session, and the
/// transcript's latest `usage` — unless `real` is false (session-start: no
/// round completed yet, so the transcript's past tells nothing).
pub(crate) fn hook_meta<'a>(c: &'a Ctx, ask: Option<&'a str>, real: bool) -> UsageMeta<'a> {
    UsageMeta {
        ask,
        session: session_meta(&c.session),
        agent: (!c.agent.is_empty()).then_some(c.agent.as_str()),
        real: real.then(|| transcript_usage(&c.session)).flatten(),
        said: &[],
        files: &[],
        decision: None,
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
/// them; this counts them next to the rejects. Info lines without the
/// `warning:` prefix (self-heal's `superseded <id>`) ride the same vec to the
/// agent but cost no round, so they never count here — symmetric with the
/// `rejected:` gate on rejects.
pub(crate) fn record_asks(
    client: &str,
    ask: &str,
    event: &str,
    repo: Option<&Path>,
    texts: &[String],
) {
    for t in texts {
        if ask == ASK_WARN && !t.starts_with("warning:") {
            continue;
        }
        record_ask(client, ask, event, repo, t);
    }
}

/// One MCP tool result: a reject (always `rejected:`-prefixed — anything else
/// is fael's own failure) or a warning line the tool sends back (`warning:`-
/// prefixed; self-heal info lines pass through uncounted, like above).
pub(crate) fn record_mcp(root: &Path, tool: &str, ask: &str, text: &str) {
    if ask == ASK_WARN {
        if !text.starts_with("warning:") {
            return;
        }
    } else if !text.starts_with("rejected:") {
        return;
    }
    record_ask("mcp", ask, tool, Some(root), text);
}

/// One ask the agent has to answer: a reject or a warning line (a stop-hook
/// block in usage rows written before that mode was removed). `repo` is `None`
/// when the call never resolved one (parse errors before routing) — those rows
/// still count, they just skip the temp-dir filter and the per-repo joins.
pub(crate) fn record_ask(client: &str, ask: &str, event: &str, repo: Option<&Path>, text: &str) {
    append_row(ask_row(client, ask, event, repo, text));
}

/// Warnings on a row just filed (`add`, `mcp-add`): like `record_asks`, plus
/// the row id under `row` (not `ids` — those count as pushes) and its writer
/// session, so add-side asks join the row and the session that wrote it.
pub(crate) fn record_row_asks(
    client: &str,
    event: &str,
    root: &Path,
    row: &core::Row,
    texts: &[String],
) {
    for t in texts.iter().filter(|t| t.starts_with("warning:")) {
        let mut v = ask_row(client, ASK_WARN, event, Some(root), t);
        v["row"] = row.id.as_str().into();
        if let Some(s) = row.session() {
            v["session"] = s.into();
        }
        append_row(v);
    }
}

fn ask_row(
    client: &str,
    ask: &str,
    event: &str,
    repo: Option<&Path>,
    text: &str,
) -> serde_json::Value {
    let mut v = serde_json::json!({
        "ts": now_rfc3339().unwrap_or_default(),
        "repo": repo.map(|r| r.to_string_lossy().into_owned()).unwrap_or_default(),
        "client": client,
        "event": event,
        "ask": ask,
        "bytes": text.len(),
        "est_tokens": core::est_tokens(text),
    });
    // an id that names no row: what the say-gate revert check counts per
    // session-start, apart from every other reject
    if text.starts_with(core::NO_ROW_WITH_ID) {
        v["reason"] = "unknown-id".into();
    }
    v
}

/// Bytes the agent pays every session before saying anything: the bundled
/// SKILL.md plus the MCP tool schemas — both local, no fetch. The chunk-6
/// ceiling test pins these; stats shows them so the cut is verifiable.
///
/// SKILL.md is normalised to LF before measuring so the count is identical
/// on every checkout — Windows git autocrlf would otherwise inflate it
/// (CRLF) vs LF checkouts and break the stats golden tests there.
pub(crate) fn constants() -> (usize, usize, usize, usize) {
    const SKILL: &str = include_str!("../../skill/SKILL.md");
    let skill = SKILL.replace("\r\n", "\n");
    let schema = crate::schema::schema_json();
    (
        skill.len(),
        core::est_tokens(&skill),
        schema.len(),
        core::est_tokens(&schema),
    )
}
