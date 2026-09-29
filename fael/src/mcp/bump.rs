//! The MCP `bump` tool — CLI-first and unlisted, still answered for clients
//! that call it. Split out of mcp.rs at the 400-line ratchet.

use super::args::{done, need, repo_for, s};
use crate::hook::{ASK_REJECT, ASK_WARN, record_asks, record_mcp};
use crate::{Repo, core, read};
use serde_json::Value;

pub(super) fn bump(a: &Value) -> Result<String, String> {
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
