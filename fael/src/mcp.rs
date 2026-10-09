//! `fael mcp` — MCP over stdio: newline-delimited JSON-RPC 2.0, three listed tools (find · add · close).
//! `bump` is CLI-first: unlisted to save schema tokens every session, still answered for clients that call it.
//! Blocking std I/O, one request at a time — no async runtime on this path (PLAN §4).
//! Tool failures come back as `isError` results so the agent reads the fix; only protocol
//! faults are JSON-RPC errors.
//!
//! Split into mcp.rs + mcp/ at the 400-line ratchet — one file per tool, this
//! file keeps only the JSON-RPC framing.

mod add;
mod args;
mod bump;
mod close;
mod find;

use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

const VERSION: &str = "2025-06-18";

/// `fael mcp --pin`: every call acts on the server's own cwd — no `cwd` arg, no
/// routing by absolute `files`. For a server reachable over HTTP (a proxy, a
/// tunnel), where any caller could otherwise read every repo on the host.
static PIN: AtomicBool = AtomicBool::new(false);

pub(crate) fn pinned() -> bool {
    PIN.load(Ordering::Relaxed)
}

/// The binary this server started from, as it stood then (01M4F5N7M).
static EXE: OnceLock<Option<Stamp>> = OnceLock::new();
/// The replaced-binary line went out — it is said once per server.
static TOLD: AtomicBool = AtomicBool::new(false);

type Stamp = (PathBuf, SystemTime);

fn stamp() -> Option<Stamp> {
    let p = std::env::current_exe().ok()?;
    let at = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
    Some((p, at))
}

/// The file is gone (brew dropped the old Cellar dir; Linux reads it as
/// `… (deleted)`) or rewritten in place since the stamp was taken.
fn replaced((p, at): &Stamp) -> bool {
    std::fs::metadata(p).and_then(|m| m.modified()).ok() != Some(*at)
}

/// Once per server, when its binary was replaced after it started: the
/// session's MCP is now an older fael than its CLI and hooks, and nothing
/// else would say so.
fn replaced_line() -> Option<String> {
    let s = EXE.get()?.as_ref()?;
    if !replaced(s) || TOLD.swap(true, Ordering::Relaxed) {
        return None;
    }
    Some(format!(
        "fael: this MCP server still runs fael {} — the installed fael was replaced since it started; restart the fael MCP server (or start a new session) so MCP matches the CLI and hooks",
        env!("CARGO_PKG_VERSION")
    ))
}

pub fn serve(pin: bool) -> Result<(), String> {
    PIN.store(pin, Ordering::Relaxed);
    EXE.get_or_init(stamp);
    // this process's env is inherited at spawn, never per call: a row stamp
    // must come from a recorded session, not whatever id it was born with
    crate::session::mark_mcp_server();
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| format!("stdin: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(&line) {
            writeln!(out, "{reply}")
                .and_then(|()| out.flush())
                .map_err(|e| format!("stdout: {e}"))?;
        }
    }
    Ok(())
}

/// One incoming line → the reply line, or None for a notification.
fn handle(line: &str) -> Option<Value> {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Some(error(Value::Null, -32700, "parse error"));
    };
    // no id = notification (notifications/initialized, cancelled, …) — never answered
    let id = msg.get("id")?.clone();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            // ponytail: echo the client's version — the tools use nothing version-specific
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(VERSION),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "fael", "version": env!("CARGO_PKG_VERSION")},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": crate::schema::tools()}),
        "tools/call" => call(&params),
        m => return Some(error(id, -32601, &format!("method not found: {m}"))),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn error(id: Value, code: i32, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

fn call(p: &Value) -> Value {
    let name = p["name"].as_str().unwrap_or("");
    let args = &crate::synonyms::mcp(name, &p["arguments"]);
    let res = match name {
        "find" => find::find(args),
        "add" => add::add(args),
        "close" => close::close(args),
        "bump" => bump::bump(args),
        n => Err(format!("unknown tool {n} — fael has find, add, close")),
    };
    let root = args::repo_for(args).ok().map(|r| r.root);
    crate::hook::record_mcp_call(name, args, root.as_deref(), &res);
    let (mut text, is_error) = match res {
        Ok(t) => (t, false),
        Err(e) => (e, true),
    };
    // first, not after a long find: a stale server is read before its output
    if let Some(l) = replaced_line() {
        text = format!("{l}\n{text}");
    }
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

#[cfg(test)]
mod tests {
    use super::replaced;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_binary_gone_or_rewritten_is_replaced() {
        let p = std::env::temp_dir().join(format!("fael-exe-{}", std::process::id()));
        std::fs::write(&p, "v1").unwrap();
        let at = std::fs::metadata(&p).unwrap().modified().unwrap();
        let s = (p.clone(), at);
        assert!(!replaced(&s));
        // rewritten in place (an installer copying over it)
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        assert!(replaced(&s));
        // removed (brew upgrade deleting the old Cellar dir)
        std::fs::remove_file(&p).unwrap();
        assert!(replaced(&s));
    }
}
