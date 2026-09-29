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
pub(super) fn repo_for(a: &Value) -> Result<Repo, String> {
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
