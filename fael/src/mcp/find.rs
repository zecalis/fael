//! The MCP `find` tool — the same rows as the CLI, ids just shown marked seen.
//! Split out of mcp.rs at the 400-line ratchet.

use super::args::{files, repo_for, s, unknown_note, with_notes};
use crate::{Repo, aliases, core};
use serde_json::Value;

/// Hidden `find` properties: read but not offered first (schema stays small,
/// agents find them through the note when they guess). Declared beside KNOWN
/// so a new schema prop lands with its entry here (or fails the test).
#[allow(dead_code)] // test-only: documents which KNOWN entries are hidden from the schema
pub(crate) const HIDDEN: &[&str] = &["since", "by", "to", "revisit", "recall"];
/// Every key `find` reads: the schema surface plus HIDDEN plus routing.
pub(crate) const KNOWN: &[&str] = &[
    "id", "ids", "text", "files", "key", "kind", "full", "all", "limit", "offset", "since", "by",
    "to", "revisit", "recall",
];

pub(super) fn find(a: &Value) -> Result<String, String> {
    let extra = unknown_note(a, KNOWN);
    let res = find_checked(a).map(|(text, _)| text);
    with_notes(res, vec![extra])
}

fn find_checked(a: &Value) -> Result<(String, Vec<String>), String> {
    let r = repo_for(a)?;
    let (text, shown) = find_inner(a, &r)?;
    // chunk 6e: ids just shown are already in this session's context
    let ids: Vec<&str> = shown.iter().map(String::as_str).collect();
    crate::hook::note_seen(&crate::session::hook_session(&r.root), &r.root, &ids);
    // `id` or `ids: [...]` pulls bodies: the ids shown, never what was typed
    let id = (s(a, "id").is_some() || a["ids"].is_array()).then(|| shown.join(","));
    let q = (s(a, "key"), files(a), id);
    crate::hook::record_found(
        "mcp",
        "mcp-find",
        &r.root,
        &text,
        &shown,
        (q.0.as_deref(), &q.1, q.2.as_deref()),
    );
    Ok((text, shown))
}

fn find_inner(a: &Value, r: &Repo) -> Result<(String, Vec<String>), String> {
    let (mut log, branch_of) = crate::journal::read(r);
    crate::hook::mark_dormant(r, &mut log);
    if let Some(ids) = a["ids"].as_array() {
        return ids_call(ids, r, log, branch_of);
    }
    // `find {"id": ...}` pulls that row's body — an id-shaped query is an id
    // lookup, never text (same messages as the CLI); pass it as `text` for a
    // text search
    if let Some(id) = s(a, "id")
        && core::looks_like_id(&id)
    {
        return match crate::refs::resolve_wide(&log, &id) {
            crate::refs::Wide::One(row) => {
                let shown = row.id.clone();
                let text = crate::journal::tag(
                    core::render_full(&log, &[row.as_ref()], 10_000),
                    &branch_of,
                );
                Ok((with_mentioned(&log, row.as_ref(), text), vec![shown]))
            }
            crate::refs::Wide::Many(rows) => Err(crate::find::reject_many(&id, &rows)),
            crate::refs::Wide::Missing => missed(r, &log, &id),
        };
    }
    // a non-id-shaped `id` keeps the old prefix shortcut: an exact id or
    // unique prefix pulls that row's body, else the call is rejected
    if let Some(id) = s(a, "id") {
        let row = core::resolve_row(&log, &id)?;
        let shown = row.id.clone();
        let text = crate::journal::tag(core::render_full(&log, &[row], 10_000), &branch_of);
        return Ok((with_mentioned(&log, row, text), vec![shown]));
    }
    // `text: "translate --all"`: a CLI flag typed into the words is no word
    let text = s(a, "text").map(|t| {
        let words: Vec<&str> = t
            .split_whitespace()
            .filter(|w| !w.starts_with("--"))
            .collect();
        words.join(" ")
    });
    let text = text.filter(|t| !t.is_empty());
    // `text: "plan:x"` reads the anchor the way `kickoff` does, `text: "#k"` the key (CLI parity)
    let (text, anchor, text_key) = crate::find::text_or_anchor(&log, None, text.as_ref(), r);
    let mut files = core::normalize_files(&files(a), &r.cwd, &r.root)?;
    files.extend(anchor);
    let revisit = revisit_arg(a);
    let f = core::Filter {
        text: text.cloned(),
        files: aliases::load(r, &log, true).expand_all(&files),
        key: s(a, "key").or(text_key),
        kind: s(a, "kind"),
        since: s(a, "since"),
        by: s(a, "by"),
        to: s(a, "to").map(|t| t.trim().to_lowercase()),
        revisit,
        recall: core::parse_recall(s(a, "recall").as_deref())?,
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
    // under full=true bodies fill the budget in a few rows: ask for the rest in
    // one call (an explicit limit beats the budget, 01M3S6GD)
    let rest_in_one = a["full"].as_bool().unwrap_or(false) && f.limit.is_none();
    let next = |n: usize| match rest_in_one {
        true => format!("offset={n} limit={}", total.saturating_sub(n)),
        false => format!("offset={n}"),
    };
    let cut = core::Cut {
        total,
        offset: f.offset,
        next: &next,
    };
    // only what fit the budget was said — like the push, count the shown lines
    // a list of one or two shows its bodies: the next call would be `find id=<id>`
    let full = a["full"].as_bool().unwrap_or(false) || core::expands(&rows, total, &f, budget);
    let text = if rows.is_empty() {
        let (head, near) = crate::find::misses::explain(&r.root, "mcp", &log, &f, "files");
        let full = full || core::expands(&near, near.len(), &f, budget);
        let body = match (near.is_empty(), full) {
            (true, _) => String::new(),
            (false, true) => core::render_full(&log, &near, budget),
            (false, false) => core::render(&log, &near, budget),
        };
        let shown = near.iter().map(|r| r.id.clone());
        let n = body.lines().filter(|l| l.starts_with("- [")).count();
        let body = crate::journal::tag(body, &branch_of);
        return Ok((
            with_hint(format!("{head}\n{body}"), &f),
            shown.take(n).collect(),
        ));
    } else {
        // issues also carry the git-proven marks, like the CLI list
        let is_issue = f.kind.as_deref() == Some("issue");
        let (branch_of, gone) = crate::find::issue::marks(r, &log, is_issue, &rows, branch_of);
        let page = match full {
            true => core::render_full_page(&log, &rows, budget, cut),
            false => core::render_page(&log, &rows, budget, cut),
        };
        crate::find::issue::tag_gone(&crate::journal::tag(page, &branch_of), &gone)
    };
    let n = text.lines().filter(|l| l.starts_with("- [")).count();
    let shown: Vec<String> = rows.iter().take(n).map(|r| r.id.clone()).collect();
    Ok((with_hint(text, &f), shown))
}

/// `revisit: true` = any revisit, a string narrows to it (CLI `--revisit[=text]`).
fn revisit_arg(a: &serde_json::Value) -> Option<String> {
    match (a["revisit"].as_bool(), s(a, "revisit")) {
        (_, Some(v)) => Some(v),
        (Some(true), None) => Some(String::new()),
        _ => None,
    }
}

/// Regex metachars are literal AND here, same hint as the CLI (MCP: `key`).
fn with_hint(text: String, f: &core::Filter) -> String {
    match core::regex_hint(f.text.as_deref()) {
        Some(h) => format!("{text}\n{h}"),
        None => text,
    }
}

/// `ids: [...]` pulls several bodies in one call under the find budget; a bad
/// id reports alone, the rest still print, and any bad id makes the call an
/// error (like batch close).
fn ids_call(
    ids: &[Value],
    r: &Repo,
    base: core::Log,
    jtags: crate::journal::BranchMap,
) -> Result<(String, Vec<String>), String> {
    let ids: Vec<String> = ids
        .iter()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();
    if ids.is_empty() {
        return Err("rejected: ids is empty — pass at least one id".into());
    }
    let spell = |left: &[String]| format!("ids=[{}]", left.join(", "));
    let p = crate::find::many::pull(r, base, jtags, &ids, false, &spell);
    let text = [p.text.trim_end().to_string()]
        .into_iter()
        .chain(p.errors.iter().cloned())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    match p.errors.is_empty() {
        true => Ok((text, p.shown)),
        false => Err(text),
    }
}

/// An id-shaped query with no row here: its chunk brief, the row where it
/// lives (one owner only), else the miss with where fael looked.
fn missed(r: &Repo, log: &core::Log, id: &str) -> Result<(String, Vec<String>), String> {
    match crate::find::owner::chunk_here(r, id) {
        Some(c) => Ok((c, vec![])),
        None => match crate::find::owner::row_elsewhere(r, id) {
            Some(w) => followed(&w),
            None => Err(crate::find::reject_missing(r, log, id)),
        },
    }
}

/// The row lives in exactly one other repo on this machine (ULIDs never
/// collide, so one hit is the row): read it there instead of rejecting.
/// Writes never follow — only this read does.
fn followed(w: &crate::find::owner::Elsewhere) -> Result<(String, Vec<String>), String> {
    let o = crate::repo_at(&w.at).map_err(|e| format!("rejected: {e}"))?;
    let (log, tags) = crate::journal::read(&o);
    let row = core::resolve_row(&log, &w.id).map_err(|e| format!("rejected: {e}"))?;
    let body = crate::journal::tag(core::render_full(&log, &[row], 10_000), &tags);
    let text = format!(
        "in {} ({}) — pass {{\"cwd\": \"{}\"}} to work there\n{}",
        w.at.display(),
        w.kind,
        w.at.display(),
        with_mentioned(&log, row, body)
    );
    Ok((text, vec![row.id.clone()]))
}

/// Rows that merely mention `row`'s id ride under its body, so the reader
/// sees who cites it — silent when nobody does (CLI `find <id>` parity).
fn with_mentioned(log: &core::Log, row: &core::Row, mut text: String) -> String {
    let who = crate::find::mentions::mentioners(log, row);
    if !who.is_empty() {
        text.push_str(&format!("mentioned by: {}\n", who.join(", ")));
    }
    text
}
