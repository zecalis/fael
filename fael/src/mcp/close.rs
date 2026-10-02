//! The MCP `close` tool — one `id`, or `ids: [...]` sharing one reason. Split
//! out of mcp.rs at the 400-line ratchet.

use super::args::{done, need, repo_for};
use crate::hook::{ASK_REJECT, ASK_WARN, record_asks, record_mcp};
use crate::write::close_row;
use serde_json::Value;

pub(super) fn close(a: &Value) -> Result<String, String> {
    let r = repo_for(a)?;
    let why = need(a, "text")?;
    // `ids: [...]` closes many in one call, like the CLI's `close a b "why"`:
    // a bad id reports alone, the rest save, any rejection makes the call an error
    let ids: Vec<String> = match a["ids"].as_array() {
        Some(v) if v.is_empty() => {
            return Err("rejected: ids is empty — pass at least one id".into());
        }
        Some(v) => v
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        None => vec![need(a, "id")?],
    };
    let (mut out, mut failed) = (vec![], 0);
    for id in &ids {
        match close_row(&r, id, &why) {
            Err(e) => {
                failed += 1;
                record_mcp(&r.root, "mcp-close", ASK_REJECT, &e);
                // one id keeps the plain error, as before
                out.push(match ids.len() {
                    1 => e,
                    _ => format!("rejected: {id}: {}", e.trim_start_matches("rejected: ")),
                });
            }
            Ok((row, _, warns)) => {
                record_asks("mcp", ASK_WARN, "mcp-close", Some(&r.root), &warns);
                out.push(done(&row.id, &warns));
            }
        }
    }
    match failed {
        0 => Ok(out.join("\n")),
        _ => Err(out.join("\n")),
    }
}
