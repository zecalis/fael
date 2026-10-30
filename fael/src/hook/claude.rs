//! `claude` + `codex` adapters: the same stdin fields and reply JSON as the
//! neutral protocol, only parsed from each client's shape and rendered back
//! into it. Codex hands stop its last message and edits as apply_patch.

use super::protocol::Event;
use super::say_card::At;
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
    #[serde(default)]
    hook_event_name: Option<String>,
    #[serde(default)]
    agent_transcript_path: Option<String>,
    #[serde(default)]
    agent_type: Option<String>,
    /// Both clients supply agent_id on child tool events too.
    #[serde(default)]
    agent_id: Option<String>,
    /// SessionStart: startup | resume | clear | compact
    #[serde(default)]
    source: Option<String>,
    /// The model the client ran this turn on, when the client names one —
    /// else the transcript tail says it (askstats::transcript_model).
    #[serde(default)]
    model: Option<String>,
}

impl ClaudeBase {
    fn subagent(&self, codex: bool) -> bool {
        codex
            && matches!(
                self.hook_event_name.as_deref(),
                Some("SubagentStart" | "SubagentStop")
            )
    }

    fn child_context(&self, codex: bool) -> bool {
        self.subagent(codex) || (codex && (self.agent_id.is_some() || self.agent_type.is_some()))
    }

    /// Codex child hooks name the parent as session_id. agent_id joins the
    /// child's shell CODEX_THREAD_ID; tool transcript_path names the child.
    fn event(self, codex: bool) -> Event {
        let nonempty = |v: Option<String>| v.filter(|s| !s.is_empty());
        let child = self.child_context(codex);
        let lifecycle = self.subagent(codex);
        let (session, transcript, agent) = if child {
            let transcript = if lifecycle {
                self.agent_transcript_path
            } else {
                self.transcript_path
            };
            (nonempty(self.agent_id), transcript, None)
        } else if codex {
            // A rollout stem is rollout-<timestamp>-<thread-id>, not the id.
            (
                nonempty(self.session_id).or_else(|| nonempty(self.transcript_path.clone())),
                self.transcript_path,
                None,
            )
        } else {
            (
                nonempty(self.transcript_path).or_else(|| nonempty(self.session_id)),
                None,
                self.agent_id,
            )
        };
        Event {
            session,
            transcript,
            agent,
            cwd: self.cwd,
            source: self.source,
            model: self.model,
            ..Event::default()
        }
    }
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
    let codex = client.as_deref() == Some("codex");
    let e = Event {
        text: Some(p.prompt),
        client,
        ..p.base.event(codex)
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
            let child = p.base.child_context(codex);
            let e = Event {
                // Codex transcripts are not Claude format. An absent last
                // message stays empty, never parsed as a Claude transcript.
                text: codex.then(|| p.last_assistant_message.clone().unwrap_or_default()),
                reply: p.last_assistant_message,
                client,
                ..p.base.event(codex)
            };
            let r = if child {
                super::stop::subagent_stop(&e)
            } else {
                stop(&e)
            };
            if let Some(n) = user_line(&r, codex) {
                println!("{}", serde_json::json!({"systemMessage": n}));
            }
            ExitCode::SUCCESS
        }
        "session-start" => {
            let p: ClaudeBase = serde_json::from_str(stdin).unwrap_or_default();
            let child = p.subagent(codex);
            let e = Event {
                client,
                ..p.event(codex)
            };
            print_reply(
                if child {
                    "SubagentStart"
                } else {
                    "SessionStart"
                },
                session_start(&e),
                codex,
            );
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
                files,
                client,
                ..p.base.event(codex)
            };
            super::cited::note_tool(&e, input);
            // a read says nothing: rows reach the agent at the edit, at kickoff and in briefs
            let reply = match event {
                "search" => super::search::push_call(&e, &p.tool_name, input, &p.tool_response),
                "edit" => push(&e, At::Edit),
                _ => super::protocol::Reply::default(),
            };
            print_reply("PostToolUse", reply, codex);
            ExitCode::SUCCESS
        }
        // ReportFindings (Claude Code only): each finding as a ready `add issue`
        "review" => {
            let p: ClaudeTool = serde_json::from_str(stdin).unwrap_or_default();
            let e = Event {
                client,
                ..p.base.event(codex)
            };
            print_reply(
                "PostToolUse",
                super::review::report_reply(&e, &p.tool_input),
                codex,
            );
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "fael hook: unknown event {event:?} — want stop|session-start|read|edit|search|prompt|review"
            );
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod identity_tests {
    use super::ClaudeBase;

    #[test]
    fn codex_child_identity_never_falls_back_to_the_parent() {
        let p: ClaudeBase = serde_json::from_value(serde_json::json!({
            "session_id":"parent", "transcript_path":"/parent/rollout.jsonl",
            "hook_event_name":"SubagentStop", "agent_id":"child",
            "agent_transcript_path":"/child/rollout.jsonl"
        }))
        .unwrap();
        let e = p.event(true);
        assert_eq!(e.session.as_deref(), Some("child"));
        assert_eq!(e.transcript.as_deref(), Some("/child/rollout.jsonl"));
        assert!(e.agent.is_none()); // The normalized child context joins its shell thread id.
        for agent in [serde_json::Value::Null, serde_json::json!("")] {
            let p: ClaudeBase = serde_json::from_value(serde_json::json!({
                "session_id":"parent", "transcript_path":"/parent/rollout.jsonl",
                "hook_event_name":"SubagentStart", "agent_id":agent
            }))
            .unwrap();
            let e = p.event(true);
            assert!(e.session.is_none());
            assert!(e.transcript.is_none());
        }
    }
}
