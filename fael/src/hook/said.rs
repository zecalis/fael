//! What a push writes back about itself: the session's seen list (row ids said,
//! `~<key>` hints spent, `@<id>` in-context marks), the 0-byte in-context
//! usage row, and the reply for a push with no rows to join.

use super::asks::hook_meta;
use super::changed::Hint;
use super::protocol::Reply;
use super::usage::{record_usage_shadow, usage_row};
use crate::core;
use std::collections::HashSet;
use std::io::Write;

/// The usage event `fael-core::stats` reads as "in context at edit".
const IN_CONTEXT: &str = "in-context";

/// Append what this push said to the session's seen list: the row ids, and a
/// `~<key>` line for each row or clause the edit hint spent (so the next edit
/// does not name it again).
pub(crate) fn remember(seen: Option<std::fs::File>, shown: &[String], hint: Option<&Hint>) {
    let Some(mut f) = seen else { return };
    let spent = hint.map(|h| h.spent.as_slice()).unwrap_or_default();
    let out: String = shown
        .iter()
        .map(|id| format!("{id}\n"))
        .chain(spent.iter().map(|k| format!("~{k}\n")))
        .collect();
    let _ = f.write_all(out.as_bytes());
}

/// Lines with no rows to join (a retire for a row already in context, a
/// stashed line) still get said on their own.
pub(crate) fn say_only(
    c: &super::protocol::Ctx,
    event: &str,
    seen: Option<std::fs::File>,
    hint: Option<Hint>,
    notes: Option<String>,
) -> Reply {
    remember(seen, &[], hint.as_ref());
    let lines: Vec<String> = hint.map(|h| h.text).into_iter().chain(notes).collect();
    if lines.is_empty() {
        return Reply::default();
    }
    let context = lines.join("\n");
    let meta = hook_meta(c, None, true);
    record_usage_shadow(&c.client, event, &c.repo.root, &context, &[], &meta, None);
    Reply {
        context: Some(context),
        ..Reply::default()
    }
}

/// PLAN-fael-visible-secretary chunk 5: decisions and issues about this very
/// file (tier 0) already in the agent's context when it edited it. Its own
/// 0-byte usage line under `in_context`, never `ids` (nothing was pushed).
/// The seen list also holds rows the agent filed or found itself, so stats
/// counts only ids an earlier push of the session handed over. Each id once
/// per session: an `@<id>` line in the seen list (never a row id) marks it.
pub(crate) fn record_in_context(
    c: &super::protocol::Ctx,
    tiered: &[(&core::Row, usize)],
    seen: &HashSet<&str>,
    f: &mut std::fs::File,
) {
    let ids: Vec<&str> = tiered
        .iter()
        .filter(|(r, tier)| {
            *tier == 0
                && matches!(r.kind.as_str(), "decision" | "issue")
                && seen.contains(r.id.as_str())
                && !seen.contains(format!("@{}", r.id).as_str())
        })
        .map(|(r, _)| r.id.as_str())
        .collect();
    if ids.is_empty() {
        return;
    }
    let marks: String = ids.iter().map(|id| format!("@{id}\n")).collect();
    let _ = f.write_all(marks.as_bytes());
    let meta = hook_meta(c, None, false);
    let mut row = usage_row(&c.client, IN_CONTEXT, &c.repo.root, "", &[], &meta);
    row["in_context"] = ids.into();
    super::asks::append_row(row);
}
