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
    // chunk 6d: one short sentence — it repeats on every tool, so every word is paid three times
    let cwd =
        str_("repo this call is about — pass when outside the session cwd, or rows land wrong");
    let mut t = json!([
        {
            "name": "find",
            "description": "Memory git lacks: why, what was rejected, open issues, unfinished work. No args = the session brief; call it first only if no fael hook gave one.",
            "annotations": {"readOnlyHint": true},
            "inputSchema": {"type": "object", "properties": {
                "id": str_("exact id or prefix, pulls the body — id-shaped with no row rejects; pass it as text for a text search"),
                "full": {"type": "boolean", "description": "bodies under titles"},
                "files": files("paths, dirs, globs, anchors like doc:pricing — rows on any"),
                "text": str_("all words, any order, in text or title; also the search for an id-shaped string"),
                "key": str_("key glob, e.g. auth:*"),
                "kind": str_("decision | issue | note, or a repo kind"),
                "since": str_("yyyy-mm or yyyy-mm-dd"),
                "to": str_("rows routed to someone, e.g. ploy"),
                "by": str_("writer who filed the row, e.g. ploy"),
                "all": {"type": "boolean", "description": "closed and superseded rows too"},
                "revisit": {"type": ["boolean", "string"], "description": "true = any revisit, a string narrows it"},
                "branches": {"type": "boolean", "description": "unmerged branches too, tagged @branch; none under store=local"},
                "limit": {"type": "integer", "minimum": 1, "description": "max rows; a cut prints next: offset=N"},
                "offset": {"type": "integer", "minimum": 0, "description": "skip this many first"},
            }},
        },
        {
            "name": "add",
            "description": "File what the next session needs — a decision and why, a bug (kind issue), or state it needs (note). Add it in the same message as your next tool call or final edit — never as a turn of its own. Write rows in English — title, key and body. The dev reads them through you, in their language. Reuse an anchor find showed, never invent one. rows[] files many at once.",
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
                "dry_run": {"type": "boolean"},
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
    ]);
    for tool in t.as_array_mut().unwrap() {
        tool["inputSchema"]["properties"]["cwd"] = cwd.clone();
    }
    t
}

#[cfg(test)]
mod tests {
    /// Chunk 6d ceiling: SKILL.md + the served schema stay under 6100 bytes
    /// combined (measured 6006 on 2026-09-30 — SKILL 2339 + schema 3667, after
    /// the reply-capture syntax replaced the Stop-hook paragraph and the
    /// doctor/MCP lines left — `doctor --help` and `tools/list` carry them;
    /// the ≥40% cut retargeted to ≥15% + a ceiling by owner decision —
    /// PLAN-fael-durable-log §3). The ceiling sits just above the measure so
    /// any growth fails here, not only in the stats golden.
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
            total <= 6100,
            "constants {total} B exceed the 6100 B ceiling — trim, don't grow"
        );
        assert!(
            total * 100 <= BASELINE * 85,
            "constants {total} B kept less than 15% off the {BASELINE} B baseline"
        );
    }
}
