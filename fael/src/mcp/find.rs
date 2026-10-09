//! The MCP `find` tool — the same rows as the CLI, ids just shown marked seen.
//! Split out of mcp.rs at the 400-line ratchet.

use super::args::{files, repo_for, s};
use crate::hook::{ASK_REJECT, record_mcp};
use crate::{Repo, aliases, core};
use serde_json::Value;

pub(super) fn find(a: &Value) -> Result<String, String> {
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
            // `id` or `ids: [...]` pulls bodies: the ids shown, never what was typed
            let id = (s(a, "id").is_some() || a["ids"].is_array()).then(|| shown.join(","));
            let q = (s(a, "key"), files(a), id);
            crate::hook::record_found(
                "mcp",
                "mcp-find",
                &r.root,
                &shown,
                (q.0.as_deref(), &q.1, q.2.as_deref()),
            );
            Ok(text)
        }
    }
}

fn find_inner(a: &Value, r: &Repo) -> Result<(String, Vec<String>), String> {
    // `branches: true` merges unmerged branches' rows into the union log
    // (HEAD wins on duplicate ids); their rows render with ` @<branch>`
    let (base, jtags) = crate::journal::read(r);
    if let Some(ids) = a["ids"].as_array() {
        return ids_call(ids, r, base, jtags);
    }
    // `find {"id": ...}` pulls that row's body — an id-shaped query is an id
    // lookup, never text (same messages as the CLI); pass it as `text` for a
    // text search
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
                Ok((with_mentioned(&log, row.as_ref(), text), vec![shown]))
            }
            crate::refs::Wide::Many(rows) => Err(crate::find::reject_many(&id, &rows)),
            crate::refs::Wide::Missing => Err(crate::find::reject_missing(&log, &id)),
        };
    }
    let (log, branch_of, note) = if a["branches"].as_bool().unwrap_or(false) {
        crate::find::branches::widen(r, base, jtags)
    } else {
        (base, jtags, None)
    };
    // a non-id-shaped `id` keeps the old prefix shortcut: an exact id or
    // unique prefix pulls that row's body, else the call is rejected
    if let Some(id) = s(a, "id") {
        let row = core::resolve_row(&log, &id)?;
        let shown = row.id.clone();
        let text = crate::find::branches::tag(core::render_full(&log, &[row], 10_000), &branch_of);
        return Ok((with_mentioned(&log, row, text), vec![shown]));
    }
    let text = s(a, "text");
    // `text: "plan:x"` reads the anchor the way `kickoff` does (CLI parity)
    let (text, anchor) = crate::find::text_or_anchor(&log, None, text.as_ref(), r);
    let mut files = core::normalize_files(&files(a), &r.cwd, &r.root)?;
    files.extend(anchor);
    // `revisit: true` = any revisit, a string narrows to it (CLI `--revisit[=text]`)
    let revisit = match (a["revisit"].as_bool(), s(a, "revisit")) {
        (_, Some(v)) => Some(v),
        (Some(true), None) => Some(String::new()),
        _ => None,
    };
    let f = core::Filter {
        text: text.cloned(),
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
    if a["groups"].as_bool().unwrap_or(false) || auto_issue(&f, a) {
        return Ok(groups(r, &log, f, &branch_of));
    }
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
        crate::find::misses::explain(&r.root, "mcp", &log, &f, "files")
    } else if full {
        crate::find::branches::tag(core::render_full_page(&log, &rows, budget, cut), &branch_of)
    } else {
        crate::find::branches::tag(core::render_page(&log, &rows, budget, cut), &branch_of)
    };
    let n = text.lines().filter(|l| l.starts_with("- [")).count();
    let shown: Vec<String> = rows.iter().take(n).map(|r| r.id.clone()).collect();
    // the CLI's stderr line has no stderr here: it rides the result
    let text = match note {
        Some(n) => format!("{}\n{n}", text.trim_end()),
        None => text,
    };
    Ok((text, shown))
}

/// `ids: [...]` pulls several bodies in one call under the find budget; a bad
/// id reports alone, the rest still print, and any bad id makes the call an
/// error (like batch close).
fn ids_call(
    ids: &[Value],
    r: &Repo,
    base: core::Log,
    jtags: crate::find::branches::BranchMap,
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

/// The plain issue list wants the grouped answer without asking for it —
/// same rule as the CLI (`find::issue::auto_grouped` takes `Args`; MCP takes
/// the flag straight off the tool object).
fn auto_issue(f: &core::Filter, a: &Value) -> bool {
    f.kind.as_deref() == Some("issue")
        && !a["full"].as_bool().unwrap_or(false)
        && f.limit.is_none()
        && f.offset == 0
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

/// `groups: true` — every match, unpaged: half a group answers the question wrong.
fn groups(
    r: &Repo,
    log: &core::Log,
    f: core::Filter,
    branch_of: &crate::find::branches::BranchMap,
) -> (String, Vec<String>) {
    let is_issue = f.kind.as_deref() == Some("issue");
    let rows = core::find(
        log,
        &core::Filter {
            limit: None,
            offset: 0,
            ..f.clone()
        },
    );
    let text = match rows.is_empty() {
        // the flat list's reason, so an empty open list says all=true adds closed
        true => core::why_empty(log, &f, "files"),
        false if is_issue => {
            crate::find::issue::render_issue_groups(r, log, &rows, branch_of.clone())
        }
        false => crate::find::branches::tag(core::render_groups(log, &rows), branch_of),
    };
    (text, rows.iter().map(|r| r.id.clone()).collect())
}
