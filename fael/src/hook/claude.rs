//! `claude` + `codex` adapters: the same stdin fields and reply JSON as the
//! neutral protocol, only parsed from each client's shape and rendered back
//! into it. Codex hands stop its last message and edits as apply_patch.

use super::protocol::Event;
use super::{push::push, session::session_start, stop::stop};
use serde::Deserialize;
use std::process::ExitCode;

#[derive(Debug, Default, Deserialize)]
struct ClaudeBase {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    transcript_path: Option<String>,
    /// Claude Code: set only when the hook fires inside a sub-agent
    #[serde(default)]
    agent_id: Option<String>,
    /// SessionStart: startup | resume | clear | compact
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeStop {
    #[serde(flatten)]
    base: ClaudeBase,
    #[serde(default)]
    stop_hook_active: bool,
    /// the turn's final assistant message — codex always sends it, Claude Code
    /// on recent versions
    #[serde(default)]
    last_assistant_message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeTool {
    #[serde(flatten)]
    base: ClaudeBase,
    /// Read/Edit: `file_path` (NotebookEdit: `notebook_path`) · codex
    /// apply_patch: `command` is the patch text · Grep/Bash: the raw call
    #[serde(default)]
    tool_input: serde_json::Value,
    #[serde(default)]
    tool_name: String,
    /// Grep/Bash: the call's output, for `search::touched`
    #[serde(default)]
    tool_response: serde_json::Value,
}

/// Paths named by an apply_patch body (`*** Add/Update/Delete File: p`,
/// `*** Move to: p`).
fn patch_files(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|l| {
            [
                "*** Add File: ",
                "*** Update File: ",
                "*** Delete File: ",
                "*** Move to: ",
            ]
            .iter()
            .find_map(|h| l.strip_prefix(h))
        })
        .map(|p| p.trim().to_string())
        .collect()
}

/// Claude Code and Codex: same stdin fields, same reply JSON.
pub(crate) fn run(event: &str, stdin: &str, client: &str) -> ExitCode {
    let client = Some(client.to_string());
    let codex = client.as_deref() == Some("codex");
    match event {
        "stop" => {
            let p: ClaudeStop = serde_json::from_str(stdin).unwrap_or_default();
            let e = Event {
                cwd: p.base.cwd,
                session: p.base.transcript_path.or(p.base.session_id),
                stop_active: p.stop_hook_active,
                // codex transcripts are not claude-format: always hand the
                // text over, so stop never falls back to parsing the file
                text: if codex {
                    Some(p.last_assistant_message.clone().unwrap_or_default())
                } else {
                    None
                },
                // the final message, when the client hands it over (codex always;
                // Claude Code on recent versions) — else stop reads the transcript
                reply: p.last_assistant_message,
                // SubagentStop runs this arm too
                agent: p.base.agent_id,
                client,
                ..Event::default()
            };
            let r = stop(&e);
            if let Some(reason) = r.reason {
                println!(
                    "{}",
                    serde_json::json!({"decision": "block", "reason": reason})
                );
            }
            ExitCode::SUCCESS
        }
        "session-start" => {
            let p: ClaudeBase = serde_json::from_str(stdin).unwrap_or_default();
            let e = Event {
                cwd: p.cwd,
                // same key as stop/edit, or session-start's branch baseline lands
                // under a filename stop never reads (transcript path over session id)
                session: p.transcript_path.or(p.session_id),
                source: p.source,
                client,
                ..Event::default()
            };
            if let Some(ctx) = session_start(&e).context {
                println!(
                    "{}",
                    serde_json::json!({"hookSpecificOutput": {
                        "hookEventName": "SessionStart", "additionalContext": ctx}})
                );
            }
            ExitCode::SUCCESS
        }
        "read" | "edit" | "search" => {
            let p: ClaudeTool = serde_json::from_str(stdin).unwrap_or_default();
            let input = &p.tool_input;
            let files = if event == "search" {
                vec![] // push_call resolves them from the raw call
            } else if let (true, Some(patch)) = (codex, input["command"].as_str()) {
                patch_files(patch)
            } else {
                ["file_path", "notebook_path"]
                    .iter()
                    .find_map(|k| input[k].as_str())
                    .map(String::from)
                    .into_iter()
                    .collect()
            };
            let e = Event {
                cwd: p.base.cwd,
                // same key as stop, which needs the transcript path
                session: p.base.transcript_path.or(p.base.session_id),
                files,
                agent: p.base.agent_id,
                client,
                ..Event::default()
            };
            let reply = if event == "search" {
                super::search::push_call(&e, &p.tool_name, input, &p.tool_response)
            } else {
                push(&e, event)
            };
            if let Some(ctx) = reply.context {
                println!(
                    "{}",
                    serde_json::json!({"hookSpecificOutput": {
                        "hookEventName": "PostToolUse", "additionalContext": ctx}})
                );
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "fael hook: unknown event {event:?} — want stop|session-start|read|edit|search"
            );
            ExitCode::SUCCESS
        }
    }
}
