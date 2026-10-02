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
                Ok((text, vec![shown]))
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
    if a["groups"].as_bool().unwrap_or(false) {
        return Ok(groups(&log, f, &branch_of));
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
    let text = if rows.is_empty() {
        "no rows match".into()
    } else if a["full"].as_bool().unwrap_or(false) {
        crate::find::branches::tag(core::render_full_page(&log, &rows, budget, cut), &branch_of)
    } else {
        crate::find::branches::tag(core::render_page(&log, &rows, budget, cut), &branch_of)
    };
    let n = text.lines().filter(|l| l.starts_with("- [")).count();
    let shown: Vec<String> = rows.iter().take(n).map(|r| r.id.clone()).collect();
    // grouping and claiming, said where the issue list is (CLI: ISSUE_TIP)
    let unpaged = f.limit.is_none() && f.offset == 0;
    let text = if f.kind.as_deref() == Some("issue") && total > 1 && unpaged {
        format!(
            "{text}fix together: find kind=issue groups=true · working one? bump id=<id> claim=true first\n"
        )
    } else {
        text
    };
    // the CLI's stderr line has no stderr here: it rides the result
    let text = match note {
        Some(n) => format!("{}\n{n}", text.trim_end()),
        None => text,
    };
    Ok((text, shown))
}

/// `groups: true` — every match, unpaged: half a group answers the question wrong.
fn groups(
    log: &core::Log,
    f: core::Filter,
    branch_of: &crate::find::branches::BranchMap,
) -> (String, Vec<String>) {
    let rows = core::find(
        log,
        &core::Filter {
            limit: None,
            offset: 0,
            ..f
        },
    );
    let text = match rows.is_empty() {
        true => "no rows match".into(),
        false => crate::find::branches::tag(core::render_groups(log, &rows), branch_of),
    };
    (text, rows.iter().map(|r| r.id.clone()).collect())
}
