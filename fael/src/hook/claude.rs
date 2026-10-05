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
    /// the turn's final assistant message — codex always sends it, Claude Code
    /// on recent versions
    #[serde(default)]
    last_assistant_message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudePrompt {
    #[serde(flatten)]
    base: ClaudeBase,
    #[serde(default)]
    prompt: String,
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

/// The user line, Claude Code only: its `systemMessage` shows to the user
/// and never reaches the model. Codex has no such channel, so it says
/// nothing (PLAN-fael-visible-secretary §2).
fn user_line(r: &super::protocol::Reply, codex: bool) -> Option<&str> {
    (!codex).then_some(r.notice.as_deref()).flatten()
}

/// `additionalContext` for the agent, `systemMessage` for the user — each
/// only when there is one; nothing at all prints nothing.
fn print_reply(event: &str, r: super::protocol::Reply, codex: bool) {
    let mut out = serde_json::Map::new();
    if let Some(ctx) = r.context() {
        out.insert(
            "hookSpecificOutput".into(),
            serde_json::json!({"hookEventName": event, "additionalContext": ctx}),
        );
    }
    if let Some(n) = user_line(&r, codex) {
        out.insert("systemMessage".into(), n.into());
    }
    if !out.is_empty() {
        println!("{}", serde_json::Value::Object(out));
    }
}

/// The prompt arm: a changed payload must say so, not silently switch the hint
/// off (01M3XKB7H). Serde defaults every field, so a renamed or dropped
/// `prompt` parses Ok with empty text — a hook on a real turn always carries
/// the prompt, so an empty one is logged. Malformed JSON is logged too. Either
/// way the reply fails open (no hint, nothing printed).
fn prompt_reply(stdin: &str, client: Option<String>) -> super::protocol::Reply {
    let Ok(p) = serde_json::from_str::<ClaudePrompt>(stdin) else {
        eprintln!("fael hook prompt: unparsable UserPromptSubmit payload");
        return super::protocol::Reply::default();
    };
    if p.prompt.is_empty() && !stdin.trim().is_empty() {
        eprintln!("fael hook prompt: UserPromptSubmit payload has no `prompt` text — hint skipped");
    }
    let e = Event {
        cwd: p.base.cwd,
        session: p.base.transcript_path.or(p.base.session_id),
        text: Some(p.prompt),
        client,
        ..Event::default()
    };
    super::prompt::prompt(&e)
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
            if let Some(n) = user_line(&r, codex) {
                println!("{}", serde_json::json!({"systemMessage": n}));
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
            print_reply("SessionStart", session_start(&e), codex);
            ExitCode::SUCCESS
        }
        "prompt" => {
            print_reply("UserPromptSubmit", prompt_reply(stdin, client), codex);
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
                push(&e, event, event)
            };
            print_reply("PostToolUse", reply, codex);
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "fael hook: unknown event {event:?} — want stop|session-start|read|edit|search|prompt"
            );
            ExitCode::SUCCESS
        }
    }
}
