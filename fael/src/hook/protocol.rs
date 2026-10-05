//! Neutral protocol (SPEC §9) — Event in, Reply out — plus the shared
//! `Ctx` every event resolves before doing anything else.

pub(crate) use super::say::Reply;
use super::{push::push, session::session_start, stop::stop};
use crate::{Repo, core, repo_at};
use serde::Deserialize;
use std::io::Read as _;
use std::path::PathBuf;
use std::process::ExitCode;

/// Neutral Event (SPEC §9) — also the shape every adapter normalises to.
#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct Event {
    #[serde(default)]
    pub(crate) cwd: Option<String>,
    /// stop: a transcript path (birthtime = session start) or an RFC 3339
    /// start time · edit: send the same string as stop — it keys the session's
    /// edit list · read: unused (reuse suppression is the client's job)
    #[serde(default)]
    pub(crate) session: Option<String>,
    #[serde(default)]
    pub(crate) client: Option<String>,
    #[serde(default)]
    pub(crate) files: Vec<String>,
    /// search: the raw tool call, when the client forwards it instead of
    /// `files` (OpenCode's plugin sends `tool`/`tool_input`/`tool_response`;
    /// `touched` resolves the files server-side, so the rule stays in one place).
    #[serde(default)]
    pub(crate) tool: Option<String>,
    #[serde(default)]
    pub(crate) tool_input: serde_json::Value,
    #[serde(default)]
    pub(crate) tool_response: serde_json::Value,
    /// stop: the assistant's text since the session start, for the issue rule.
    /// prompt: the user's prompt.
    /// Any client that can see its own messages sends it; without it fael
    /// falls back to reading `session` as a Claude-format transcript.
    #[serde(default)]
    pub(crate) text: Option<String>,
    /// stop: the last assistant message only — the capture collector reads
    /// its `fael <kind>:` lines. Without it fael reads the last message of a
    /// Claude-format transcript in `session`.
    #[serde(default)]
    pub(crate) reply: Option<String>,
    /// read/edit/stop: the sub-agent the event fired inside, when the client
    /// names one. A sub-agent is its own context window: it keeps its own
    /// seen list, and its stop only files its reply's capture lines.
    #[serde(default)]
    pub(crate) agent: Option<String>,
    /// session-start: `"compact"` when the client just compacted its context —
    /// the rows pushed into it are gone, so the seen list starts over.
    #[serde(default)]
    pub(crate) source: Option<String>,
}

pub(crate) fn cmd(event: &str, client: Option<String>) -> ExitCode {
    let mut stdin = String::new();
    if std::io::stdin().read_to_string(&mut stdin).is_err() {
        return fail_open(event, client.as_deref());
    }
    match client.as_deref() {
        None => neutral(event, &stdin),
        Some(c @ ("claude" | "codex")) => super::claude::run(event, &stdin, c),
        Some(_) => ExitCode::SUCCESS, // unknown client: fail open, print nothing
    }
}

/// Parse broke or wrong event — still exit 0, with the shape the caller reads.
fn fail_open(_event: &str, client: Option<&str>) -> ExitCode {
    if client.is_none() {
        println!("{}", serde_json::json!({"block": false}));
    }
    ExitCode::SUCCESS
}

fn neutral(event: &str, stdin: &str) -> ExitCode {
    let e: Event = match serde_json::from_str(stdin) {
        Ok(e) => e,
        Err(_) => return fail_open(event, None),
    };
    let reply = match event {
        "stop" => stop(&e),
        "session-start" => session_start(&e),
        "read" | "edit" => push(&e, event, event),
        "search" if e.files.is_empty() => super::search::push_call(
            &e,
            e.tool.as_deref().unwrap_or(""),
            &e.tool_input,
            &e.tool_response,
        ),
        // the client named the files: how it found them is not ours to say
        "search" => push(&e, event, "search"),
        "prompt" => super::prompt::prompt(&e),
        _ => {
            eprintln!(
                "fael hook: unknown event {event:?} — want stop|session-start|read|edit|search|prompt"
            );
            Reply::default()
        }
    };
    println!(
        "{}",
        serde_json::to_string(&reply).unwrap_or(r#"{"block":false}"#.into())
    );
    ExitCode::SUCCESS
}

pub(crate) struct Ctx {
    pub(crate) repo: Repo,
    pub(crate) log: core::Log,
    /// branch tags for journal-only rows (`journal::read`), so the session
    /// brief and the read/edit push render `@<branch>` like `find` does
    pub(crate) tags: crate::find::branches::BranchMap,
    pub(crate) client: String,
    pub(crate) session: String,
    /// the sub-agent id, empty on the session's own thread
    pub(crate) agent: String,
}

/// Resolve cwd → repo + log. `None` = fail open (not a repo, no cwd, …).
pub(crate) fn ctx(e: &Event) -> Option<Ctx> {
    let cwd = e
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let repo = repo_at(&cwd).ok()?;
    let (log, tags) = crate::journal::read(&repo);
    Some(Ctx {
        repo,
        log,
        tags,
        client: e.client.clone().unwrap_or_else(|| "neutral".into()),
        session: e.session.clone().unwrap_or_default(),
        agent: e.agent.clone().unwrap_or_default(),
    })
}
