//! The MCP `close` tool — one `id`, `ids: [...]` sharing one reason, or a `key`. Split
//! out of mcp.rs at the 400-line ratchet.

use super::args::{done, need, repo_for, unknown_note, with_notes};
use crate::batch::closed_line;
use crate::close_key::{BOTH, open_row_on};
use crate::hook::{ASK_WARN, record_asks};
use crate::write::close_row;
use serde_json::Value;

/// No hidden `close` properties — KNOWN is the schema surface plus routing.
#[allow(dead_code)] // test-only: documents which KNOWN entries are hidden from the schema
pub(crate) const HIDDEN: &[&str] = &[];
/// Every key `close` reads.
pub(crate) const KNOWN: &[&str] = &["id", "ids", "key", "text", "force"];

pub(super) fn close(a: &Value) -> Result<String, String> {
    let extra = unknown_note(a, KNOWN);
    let res = close_checked(a);
    with_notes(res, vec![extra])
}

fn close_checked(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    let why = need(a, "text")?;
    // `ids: [...]` closes many in one call, like the CLI's `close a b "why"`:
    // a bad id reports alone, the rest save, any rejection makes the call an error
    let ids: Vec<String> = match (a["key"].as_str(), a["ids"].as_array()) {
        (Some(_), _) if a["id"].is_string() || a["ids"].is_array() => {
            return Err(BOTH.into());
        }
        // `key` closes the one open row on that topic; none or several close nothing
        (Some(key), _) => vec![open_row_on(&r, key)?],
        (None, Some(v)) if v.is_empty() => {
            return Err("rejected: ids is empty — pass at least one id".into());
        }
        (None, Some(v)) => v
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        (None, None) => vec![need(a, "id")?],
    };
    let (mut out, mut failed) = (vec![], 0);
    for id in &ids {
        match close_row(&r, id, &why, a["force"].as_bool().unwrap_or(false)) {
            Err(e) => {
                failed += 1;
                // one id keeps the plain error, as before
                out.push(match ids.len() {
                    1 => e,
                    _ => format!("rejected: {id}: {}", e.trim_start_matches("rejected: ")),
                });
            }
            Ok((row, _, warns)) => {
                record_asks("mcp", ASK_WARN, "mcp-close", Some(&r.root), &warns);
                out.push(done(&row.id, &warns));
                out.extend(closed_line(&r, &row));
            }
        }
    }
    match failed {
        0 => Ok(out.join("\n")),
        _ => Err(out.join("\n")),
    }
}
