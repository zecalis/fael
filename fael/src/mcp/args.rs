//! Shared argument helpers for the MCP tools — reading `arguments` and resolving
//! the repo a call acts on. Split out of mcp.rs at the 400-line ratchet.

use crate::{Repo, core, repo, repo_at};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) fn s(a: &Value, k: &str) -> Option<String> {
    a[k].as_str().filter(|v| !v.is_empty()).map(String::from)
}

pub(super) fn need(a: &Value, k: &str) -> Result<String, String> {
    s(a, k).ok_or(format!("rejected: {k} is required — pass it in arguments"))
}

pub(super) fn files(a: &Value) -> Vec<String> {
    a["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect()
}

/// The repo a call acts on. The server runs in the session's cwd, so an agent
/// working in another worktree would write there: `cwd` wins, then the repo
/// holding the first absolute `files` path, then the server's own cwd.
/// Pinned (`fael mcp --pin`): the server's own cwd only.
pub(super) fn repo_for(a: &Value) -> Result<Repo, String> {
    if super::pinned() {
        if s(a, "cwd").is_some() {
            return Err("rejected: this server is pinned to one workspace — drop cwd".into());
        }
        return repo();
    }
    if let Some(d) = s(a, "cwd") {
        return repo_at(Path::new(&d));
    }
    let abs = files(a)
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_absolute());
    // a file not created yet: its nearest existing ancestor holds the repo
    if let Some(d) = abs
        .as_deref()
        .and_then(|p| p.ancestors().find(|d| d.is_dir()))
    {
        let r = repo_at(d)?;
        // a path outside any git repo must not start a .fael/ where it lands
        if r.root.join(".git").exists() {
            return Ok(r);
        }
    }
    repo()
}

pub(super) fn done(id: &str, warns: &[String]) -> String {
    std::iter::once(format!("recorded {id}"))
        .chain(warns.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `add --urgent` over MCP: `urgent` files at the back of the queue,
/// `urgent_before` just above that row — one of the two at most.
pub(super) fn urgent_ask(a: &Value) -> Result<core::Urgent, String> {
    match (
        a["urgent"].as_bool().unwrap_or(false),
        s(a, "urgent_before"),
    ) {
        (false, None) => Ok(core::Urgent::Unset),
        (true, None) => Ok(core::Urgent::End),
        (false, Some(t)) => Ok(core::Urgent::Before(t)),
        (true, Some(_)) => Err(
            "rejected: urgent and urgent_before pick one — the queue takes a single position"
                .into(),
        ),
    }
}

/// Unknown top-level keys as a `note:` line — never a reject, so a newer
/// client sending a field this binary does not know keeps working
/// (forward-compat). The agent dropped the key silently before (opencode
/// `pr`/`left` round-trip, 2026-10-22); now it says what fell away.
pub(super) fn unknown_note(a: &Value, known: &[&str]) -> Option<String> {
    let o = a.as_object()?;
    let mut extra: Vec<&str> = o
        .keys()
        .map(String::as_str)
        .filter(|k| *k != "cwd" && !known.contains(k))
        .collect();
    extra.sort_unstable();
    extra.dedup();
    match extra.as_slice() {
        [] => None,
        [one] => Some(format!(
            "note: unknown arg `{one}` ignored — update fael if it should mean something"
        )),
        many => Some(format!(
            "note: unknown args `{}` ignored — update fael if they should mean something",
            many.join(", ")
        )),
    }
}

/// Append notes without moving the first line — an error still opens with
/// `rejected:`, so stats keep counting it.
pub(super) fn with_notes(
    mut res: Result<String, String>,
    notes: Vec<Option<String>>,
) -> Result<String, String> {
    let extra: Vec<String> = notes.into_iter().flatten().collect();
    if extra.is_empty() {
        return res;
    }
    let tail = extra.join("\n");
    match &mut res {
        Ok(t) | Err(t) => {
            if !t.is_empty() {
                t.push('\n');
            }
            t.push_str(&tail);
        }
    }
    res
}
