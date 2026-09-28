//! `fael mcp` — MCP over stdio: newline-delimited JSON-RPC 2.0, three listed tools (find · add · close).
//! `bump` is CLI-first: unlisted to save schema tokens every session, still answered for clients that call it.
//! Blocking std I/O, one request at a time — no async runtime on this path (PLAN §4).
//! Tool failures come back as `isError` results so the agent reads the fix; only protocol
//! faults are JSON-RPC errors.

use crate::hook::{ASK_REJECT, ASK_WARN, record_asks, record_mcp};
use crate::{Repo, aliases, close_row, core, read, repo, repo_at, write::AddOpts, write::add_row};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const VERSION: &str = "2025-06-18";

pub fn serve() -> Result<(), String> {
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
    let args = &p["arguments"];
    let res = match p["name"].as_str().unwrap_or("") {
        "find" => find(args),
        "add" => add(args),
        "close" => close(args),
        "bump" => bump(args),
        n => Err(format!("unknown tool {n} — fael has find, add, close")),
    };
    let (text, is_error) = match res {
        Ok(t) => (t, false),
        Err(e) => (e, true),
    };
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn s(a: &Value, k: &str) -> Option<String> {
    a[k].as_str().filter(|v| !v.is_empty()).map(String::from)
}

fn need(a: &Value, k: &str) -> Result<String, String> {
    s(a, k).ok_or(format!("rejected: {k} is required — pass it in arguments"))
}

fn files(a: &Value) -> Vec<String> {
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
fn repo_for(a: &Value) -> Result<Repo, String> {
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

fn find(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    match find_inner(a, &r) {
        Err(e) => {
            record_mcp(&r.root, "mcp-find", ASK_REJECT, &e);
            Err(e)
        }
        Ok((text, shown)) => {
            // chunk 6e: ids just shown are already in this session's context
            let ids: Vec<&str> = shown.iter().map(String::as_str).collect();
            crate::hook::note_seen(&crate::session::hook_session(&r.root), &r.root, &ids);
            Ok(text)
        }
    }
}

fn find_inner(a: &Value, r: &Repo) -> Result<(String, Vec<String>), String> {
    // `branches: true` merges unmerged branches' rows into the union log
    // (HEAD wins on duplicate ids); their rows render with ` @<branch>`
    let (base, jtags) = crate::journal::read(r);
    // `find {"id": ...}` pulls that row's body — an id-shaped query is an id
    // lookup, never text (same messages as the CLI); pass it as `text` for a
    // literal text search
    if let Some(id) = s(a, "id")
        && core::looks_like_id(&id)
    {
        let (log, wide, btags) = crate::refs::resolve_wide(r, base, &id);
        let branch_of = crate::journal::overlay(jtags, btags);
        return match wide {
            crate::refs::Wide::One(row) => {
                let shown = row.id.clone();
                let text = crate::find::branches::tag(
                    core::render_full(&log, &[row.as_ref()], 10_000),
                    &branch_of,
                );
                Ok((text, vec![shown]))
            }
            crate::refs::Wide::Many(rows) => Err(crate::find::reject_many(&id, &rows)),
            crate::refs::Wide::Missing => Err(crate::find::reject_missing(&log, &id)),
        };
    }
    let (log, branch_of) = if a["branches"].as_bool().unwrap_or(false) {
        let (log, btags) = crate::find::branches::with_branches(&r.root, base);
        (log, crate::journal::overlay(jtags, btags))
    } else {
        (base, jtags)
    };
    // a non-id-shaped `id` keeps the old prefix shortcut: an exact id or
    // unique prefix pulls that row's body, else the call is rejected
    if let Some(id) = s(a, "id") {
        let row = core::resolve(&log, &id)?;
        let shown = row.id.clone();
        let text = crate::find::branches::tag(core::render_full(&log, &[row], 10_000), &branch_of);
        return Ok((text, vec![shown]));
    }
    let files = core::normalize_files(&files(a), &r.cwd, &r.root)?;
    // `revisit: true` = any revisit, a string narrows to it (CLI `--revisit[=text]`)
    let revisit = match (a["revisit"].as_bool(), s(a, "revisit")) {
        (_, Some(v)) => Some(v),
        (Some(true), None) => Some(String::new()),
        _ => None,
    };
    let f = core::Filter {
        text: s(a, "text"),
        files: aliases::load(r, &log, true).expand_all(&files),
        key: s(a, "key"),
        kind: s(a, "kind"),
        since: s(a, "since"),
        by: s(a, "by"),
        to: s(a, "to").map(|t| t.trim().to_lowercase()),
        revisit,
        all: a["all"].as_bool().unwrap_or(false),
        limit: match a["limit"].as_u64() {
            Some(0) => {
                return Err("rejected: limit 0 shows nothing — drop it or give 1 or more".into());
            }
            n => n.map(|n| n as usize),
        },
        offset: a["offset"].as_u64().unwrap_or(0) as usize,
    };
    // same rows as the CLI: query() pages after ranking, the cut line names
    // the next offset to repeat the call with
    let (rows, budget, total) = core::query(&log, &f, &r.cfg);
    let cut = core::Cut {
        total,
        offset: f.offset,
        next: &|n| format!("offset={n}"),
    };
    // only what fit the budget was said — like the push, count the shown lines
    let text = if rows.is_empty() {
        "no rows match".into()
    } else if a["full"].as_bool().unwrap_or(false) {
        crate::find::branches::tag(core::render_full_page(&log, &rows, budget, cut), &branch_of)
    } else {
        crate::find::branches::tag(core::render_page(&log, &rows, budget, cut), &branch_of)
    };
    let n = text.lines().filter(|l| l.starts_with("- [")).count();
    let shown: Vec<String> = rows.iter().take(n).map(|r| r.id.clone()).collect();
    Ok((text, shown))
}

fn add(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    match add_inner(a, &r) {
        Err(e) => {
            record_mcp(&r.root, "mcp-add", ASK_REJECT, &e);
            Err(e)
        }
        Ok((text, warns)) => {
            record_asks("mcp", ASK_WARN, "mcp-add", Some(&r.root), &warns);
            Ok(text)
        }
    }
}

fn add_inner(a: &Value, r: &Repo) -> Result<(String, Vec<String>), String> {
    // chunk 6b: `rows: [...]` files many rows in one call — each runs the same
    // validate + self-heal as a single add; a bad row reports alone, the rest save
    if let Some(rows) = a["rows"].as_array() {
        if rows.is_empty() {
            return Err("rejected: rows is empty — pass at least one row".into());
        }
        let mut out = vec![];
        let mut warns = vec![];
        let mut failed = 0;
        for (i, v) in rows.iter().enumerate() {
            let b = crate::batch::batch_row(v).map_err(|e| row_err(e, i))?;
            match add_row(
                r,
                &b.kind,
                &b.text,
                &b.files,
                AddOpts {
                    key: b.opts.key,
                    to: b.opts.to,
                    title: b.opts.title,
                    revisit: b.opts.revisit,
                    urgent: b.opts.urgent,
                    supersedes: b.opts.supersedes,
                    force: b.opts.force,
                },
            ) {
                Ok((row, _, w)) => {
                    out.push(format!("recorded {}", row.id));
                    warns.extend(w);
                }
                Err(e) => {
                    failed += 1;
                    let e = format!("rejected: row {i}: {}", e.trim_start_matches("rejected: "));
                    record_mcp(&r.root, "mcp-add", ASK_REJECT, &e);
                    out.push(e);
                }
            }
        }
        // like the CLI batch: any rejection turns the call into an error —
        // the saved rows stay saved, their warnings ride along
        if failed > 0 {
            record_asks("mcp", ASK_WARN, "mcp-add", Some(&r.root), &warns);
            out.extend(warns);
            return Err(out.join("\n"));
        }
        // chunk 6e rides inside write::add_row — the saved ids are already seen
        return Ok((out.join("\n"), warns));
    }
    let (row, _, warns) = add_row(
        r,
        &need(a, "kind")?,
        &need(a, "text")?,
        &files(a),
        AddOpts {
            key: s(a, "key"),
            to: s(a, "to"),
            title: s(a, "title"),
            revisit: s(a, "revisit"),
            urgent: urgent_ask(a)?,
            supersedes: s(a, "supersedes"),
            force: a["force"].as_bool().unwrap_or(false),
        },
    )?;
    Ok((done(&row.id, &warns), warns))
}

fn row_err(e: String, i: usize) -> String {
    format!("rejected: row {i}: {}", e.trim_start_matches("rejected: "))
}

/// `add --urgent` over MCP: `urgent` files at the back of the queue,
/// `urgent_before` just above that row — one of the two at most.
fn urgent_ask(a: &Value) -> Result<core::Urgent, String> {
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

fn bump(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    match bump_inner(a, &r) {
        Err(e) => {
            record_mcp(&r.root, "mcp-bump", ASK_REJECT, &e);
            Err(e)
        }
        Ok((text, warns)) => {
            record_asks("mcp", ASK_WARN, "mcp-bump", Some(&r.root), &warns);
            Ok(text)
        }
    }
}

fn bump_inner(a: &Value, r: &Repo) -> Result<(String, Vec<String>), String> {
    let log = read(r);
    let urgent = match (
        a["urgent"].as_bool().unwrap_or(false),
        s(a, "urgent_before"),
        a["not_urgent"].as_bool().unwrap_or(false),
    ) {
        (false, None, false) => core::UrgentChange::Keep,
        (true, None, false) => core::UrgentChange::End,
        (false, Some(t), false) => core::UrgentChange::Before(t),
        (false, None, true) => core::UrgentChange::Remove,
        _ => return Err("rejected: urgent, urgent_before and not_urgent pick one — the queue takes a single position".into()),
    };
    let (row, _, warns) = core::bump_row(
        &r.fael,
        r.journal.as_deref(),
        &log,
        &r.cfg,
        &crate::stamp(r),
        &need(a, "id")?,
        core::BumpOpts {
            to: s(a, "to"),
            urgent,
            revisit: s(a, "revisit"),
        },
    )?;
    Ok((done(&row.id, &warns), warns))
}

fn close(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    match close_row(&r, &need(a, "id")?, &need(a, "text")?) {
        Err(e) => {
            record_mcp(&r.root, "mcp-close", ASK_REJECT, &e);
            Err(e)
        }
        Ok((row, _, warns)) => {
            record_asks("mcp", ASK_WARN, "mcp-close", Some(&r.root), &warns);
            Ok(done(&row.id, &warns))
        }
    }
}

fn done(id: &str, warns: &[String]) -> String {
    std::iter::once(format!("recorded {id}"))
        .chain(warns.iter().cloned())
        .collect::<Vec<_>>()
        .join("\n")
}
