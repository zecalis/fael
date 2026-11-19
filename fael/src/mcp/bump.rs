//! The MCP `bump` tool — CLI-first and unlisted, still answered for clients
//! that call it. Split out of mcp.rs at the 400-line ratchet.

use super::args::{done, need, repo_for, s, unknown_note, with_notes};
use crate::hook::{ASK_WARN, record_asks};
use crate::{Repo, core, read};
use serde_json::Value;

/// `bump` is CLI-first and unlisted: no schema surface, so KNOWN is the full
/// read set, declared here beside the code that reads it.
#[allow(dead_code)] // test-only: documents which KNOWN entries are hidden from the schema
pub(crate) const HIDDEN: &[&str] = &[];
/// Every key `bump` reads.
pub(crate) const KNOWN: &[&str] = &["id", "claim", "force", "why"];

pub(super) fn bump(a: &Value) -> Result<String, String> {
    let extra = unknown_note(a, KNOWN);
    let res = bump_checked(a);
    with_notes(res, vec![extra])
}

fn bump_checked(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    let (text, warns) = bump_inner(a, &r)?;
    record_asks("mcp", ASK_WARN, "mcp-bump", Some(&r.root), &warns);
    Ok(text)
}

fn bump_inner(a: &Value, r: &Repo) -> Result<(String, Vec<String>), String> {
    // `claim: true` = `fael claim <id>`: this branch holds the issue
    if a["claim"].as_bool().unwrap_or(false) {
        let (row, _, warns) =
            crate::claim::claim(r, &need(a, "id")?, a["force"].as_bool().unwrap_or(false))?;
        return Ok((done(&row.id, &warns), warns));
    }
    let log = read(r);
    let id = need(a, "id")?;
    let routing = false;
    let (fh, note) = crate::filehash::for_bump(&r.root, &log, &id, routing)?;
    let stamp = crate::session::retire_stamp(r, &log, &id);
    let (row, _, mut warns) = core::bump_row(
        &r.fael,
        r.journal.as_deref(),
        &log,
        &r.cfg,
        &stamp,
        &id,
        core::BumpOpts {
            to: None,
            urgent: core::UrgentChange::Keep,
            revisit: None,
            held: None,
            fh: Some(fh),
            why: s(a, "why"),
        },
    )?;
    warns.extend(note);
    Ok((done(&row.id, &warns), warns))
}
