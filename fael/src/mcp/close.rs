//! The MCP `close` tool. Split out of mcp.rs at the 400-line ratchet.

use super::args::{done, need, repo_for};
use crate::hook::{ASK_REJECT, ASK_WARN, record_asks, record_mcp};
use crate::write::close_row;
use serde_json::Value;

pub(super) fn close(a: &Value) -> Result<String, String> {
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
