//! The MCP tool schemas as served on `tools/list` — split out of mcp.rs
//! (file-size ratchet). `stats` sizes this same string for the per-session
//! constants, so the number it shows is the number the agent actually pays.

use serde_json::{Value, json};

/// The MCP tool schemas as served on `tools/list` — `stats` sizes the same
/// string for the per-session constants, so the number it shows is the number
/// the agent actually pays.
pub(crate) fn schema_json() -> String {
    serde_json::to_string(&tools()).unwrap_or_default()
}

pub(crate) fn tools() -> Value {
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
            "description": "File what the next session needs — a decision and why, a bug (kind issue), or state it needs (note). Add it in the same message as your next tool call or final edit — never as a turn of its own. English rows; reuse an anchor find showed, never invent one. rows[] files many at once.",
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
            "inputSchema": {"type": "object", "required": ["id"], "properties": {
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

#[cfg(test)]
mod tests {
    /// Chunk 6d ceiling: SKILL.md + the served schema stay under 6400 bytes
    /// combined (measured 6383 on 2026-09-28; the ≥40% cut retargeted to ≥15%
    /// + this ceiling by owner decision — PLAN-fael-durable-log §3).
    ///
    /// Line endings are normalized first: `include_str!` reads the checkout,
    /// and a CRLF checkout (Windows) would add one byte per line without any
    /// content growing. The ceiling guards content, so measure canonical LF.
    ///
    /// English text, so −bytes = −tokens with no recount.
    const SKILL: &str = include_str!("../skill/SKILL.md");
    const BASELINE: usize = 7756;

    #[test]
    fn constants_stay_small() {
        let total = SKILL.replace("\r\n", "\n").len() + super::schema_json().len();
        assert!(
            total <= 6400,
            "constants {total} B exceed the 6400 B ceiling — trim, don't grow"
        );
        assert!(
            total * 100 <= BASELINE * 85,
            "constants {total} B kept less than 15% off the {BASELINE} B baseline"
        );
    }
}
