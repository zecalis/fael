//! `fael mcp` — MCP over stdio: newline-delimited JSON-RPC 2.0, four tools (find · add · close · bump).
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
            // ponytail: echo the client's version — the four tools use nothing version-specific
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(VERSION),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "fael", "version": env!("CARGO_PKG_VERSION")},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
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
        n => Err(format!(
            "unknown tool {n} — fael has find, add, close, bump"
        )),
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

/// The MCP tool schemas as served on `tools/list` — `stats` sizes the same
/// string for the per-session constants, so the number it shows is the number
/// the agent actually pays.
pub(crate) fn schema_json() -> String {
    serde_json::to_string(&tools()).unwrap_or_default()
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
            crate::hook::note_seen(&crate::write::hook_session(&r.root), &r.root, &ids);
            Ok(text)
        }
    }
}

fn find_inner(a: &Value, r: &Repo) -> Result<(String, Vec<String>), String> {
    // `branches: true` merges unmerged branches' rows into the union log
    // (HEAD wins on duplicate ids); their rows render with ` @<branch>`
    let (base, jtags) = crate::journal::read(r);
    let (log, branch_of) = if a["branches"].as_bool().unwrap_or(false) {
        let (log, btags) = crate::find::branches::with_branches(&r.root, base);
        (log, crate::journal::overlay(jtags, btags))
    } else {
        (base, jtags)
    };
    // `find {"id": ...}` pulls that row's body by exact id or unique prefix —
    // lists show titles, this is how the body is read on demand
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
        to: s(a, "to").map(|t| t.trim().to_lowercase()),
        revisit,
        limit: match a["limit"].as_u64() {
            Some(0) => {
                return Err("rejected: limit 0 shows nothing — drop it or give 1 or more".into());
            }
            n => n.map(|n| n as usize),
        },
        offset: a["offset"].as_u64().unwrap_or(0) as usize,
        ..core::Filter::default()
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
        for (i, v) in rows.iter().enumerate() {
            let b = crate::write::batch_row(v).map_err(|e| row_err(e, i))?;
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
                    let e = format!("rejected: row {i}: {}", e.trim_start_matches("rejected: "));
                    record_mcp(&r.root, "mcp-add", ASK_REJECT, &e);
                    out.push(e);
                }
            }
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

fn tools() -> Value {
    let str_ = |d: &str| json!({"type": "string", "description": d});
    let files = |d: &str| json!({"type": "array", "items": {"type": "string"}, "description": d});
    // chunk 6d: one short sentence — it repeats on every tool, so every word is paid four times
    let cwd =
        str_("repo this call is about — pass when outside the session cwd, or rows land wrong");
    let mut t = json!([
        {
            "name": "find",
            "description": "Project memory: past decisions, issues, notes. No args = the session brief. Start of task, before touching a file.",
            "annotations": {"readOnlyHint": true},
            "inputSchema": {"type": "object", "properties": {
                "id": str_("exact id or prefix — lists show titles, this pulls the body"),
                "full": {"type": "boolean", "description": "bodies under titles"},
                "files": files("paths, dirs, globs, anchors like doc:pricing — rows on any"),
                "text": str_("substring of the row text"),
                "key": str_("key glob, e.g. auth:*"),
                "kind": str_("decision | issue | note, or a repo kind"),
                "since": str_("yyyy-mm or yyyy-mm-dd"),
                "to": str_("rows routed to someone, e.g. ploy"),
                "revisit": {"type": ["boolean", "string"], "description": "true = any revisit, a string narrows it"},
                "branches": {"type": "boolean", "description": "unmerged branches too, tagged @branch, no checkout"},
                "limit": {"type": "integer", "minimum": 1, "description": "max rows; a cut prints next: offset=N"},
                "offset": {"type": "integer", "minimum": 0, "description": "skip this many first"},
            }},
        },
        {
            "name": "add",
            "description": "File what the next session needs — a decision and why, a bug (kind issue), or state it needs (note). Same message as your next tool call, never its own turn. English rows; reuse an anchor find showed, never invent one. rows[] files many at once.",
            "inputSchema": {"type": "object", "required": ["kind", "text"], "properties": {
                "kind": str_("decision | issue | note, or a repo kind"),
                "text": str_("what happened and why, standalone"),
                "title": str_("≤15-word list headline — set it past ~60 words"),
                "files": {"type": "array", "items": {"type": "string"},
                    "description": "paths or scheme:ref anchors — omit for this session's edited files"},
                "rows": {"type": "array", "items": {"type": "object"},
                    "description": "batch [{kind, text, files, ...}] — a bad row reports alone, the rest save"},
                "key": str_("optional colon key, e.g. auth:session"),
                "to": str_("who answers, e.g. ploy"),
                "revisit": str_("date YYYY-MM[-DD] or free text"),
                "urgent": {"type": "boolean", "description": "back of the urgent queue (issues)"},
                "urgent_before": str_("above that row — one of urgent / urgent_before"),
                "supersedes": str_("id this replaces"),
                "force": {"type": "boolean", "description": "allow a typo-lookalike path"},
            }},
        },
        {
            "name": "close",
            "description": "Close a fixed issue or done note.",
            "inputSchema": {"type": "object", "required": ["id", "text"], "properties": {
                "id": str_("id or prefix, as find showed"),
                "text": str_("why, e.g. fixed in <sha>"),
            }},
        },
        {
            "name": "bump",
            "description": "New version of an open row with new routing — text and files never change.",
            "inputSchema": {"type": "object", "required": ["id"], "properties": {
                "id": str_("id or prefix, as find showed"),
                "to": str_("who answers now — omit to keep"),
                "urgent": {"type": "boolean", "description": "to the back of the queue"},
                "urgent_before": str_("just above that row"),
                "not_urgent": {"type": "boolean", "description": "leave the queue"},
                "revisit": str_("date or text — omit to keep"),
            }},
        },
    ]);
    for tool in t.as_array_mut().unwrap() {
        tool["inputSchema"]["properties"]["cwd"] = cwd.clone();
    }
    t
}
