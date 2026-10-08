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
    let cwd = str_("repo this call is about, if not the session cwd");
    let mut t = json!([
        {
            "name": "find",
            "description": "No args = the session brief; call it first only if no fael hook gave one.",
            "annotations": {"readOnlyHint": true},
            "inputSchema": {"type": "object", "properties": {
                "id": str_("exact id or prefix, pulls the body — id-shaped with no row rejects; pass it as text for a text search"),
                "ids": files("many ids, bodies in one call — a bad id fails alone"),
                "full": {"type": "boolean", "description": "bodies under titles"},
                "files": files("paths, dirs, globs, anchors like doc:pricing — rows on any"),
                "text": str_("all words, any order, in text or title"),
                "key": str_("key glob, e.g. auth:*"),
                "kind": str_("decision | issue | note, or a repo kind"),
                "all": {"type": "boolean", "description": "closed and superseded rows too"},
                "limit": {"type": "integer", "minimum": 1, "description": "max rows"},
                "offset": {"type": "integer", "minimum": 0, "description": "skip this many first"},
            }},
        },
        {
            "name": "add",
            "description": "File what the next session needs — a decision and why, a bug (kind issue), or state it needs (note). Add it in the same message as your next tool call or final edit — never as a turn of its own.",
            "inputSchema": {"type": "object", "required": ["kind", "text"], "properties": {
                "kind": str_("decision | issue | note, or a repo kind"),
                "text": str_("what happened and why, standalone"),
                "title": str_("≤15-word headline — set it if the first sentence passes ~12 words"),
                "files": {"type": "array", "items": {"type": "string"},
                    "description": "paths or scheme:ref anchors — omit for this session's edited files"},
                "rows": {"type": "array", "items": {"type": "object"},
                    "description": "batch [{kind, text, files, ...}] — a bad row reports alone, the rest save"},
                "key": str_("optional colon key, e.g. auth:session"),
                "to": str_("who answers, e.g. ploy"),
                "from": str_("user if the user said or decided it"),
                "revisit": str_("date YYYY-MM[-DD] or free text"),
                "urgent": {"type": "boolean", "description": "back of the urgent queue (issues)"},
                "supersedes": str_("id this replaces"),
            }},
        },
        {
            "name": "close",
            "description": "Close a fixed issue or done note.",
            "inputSchema": {"type": "object", "required": ["text"], "properties": {
                "id": str_("id or prefix from find"),
                "ids": {"type": "array", "items": {"type": "string"},
                    "description": "many ids, one reason"},
                "key": str_("the one open row on this key"),
                "text": str_("why; a fixed bug: <cause> → <fix>; tried <what failed>; guard `<test path>`"),
            }},
        },
    ]);
    // pinned: one workspace, so no `cwd` to route by
    if !crate::mcp::pinned() {
        for tool in t.as_array_mut().unwrap() {
            tool["inputSchema"]["properties"]["cwd"] = cwd.clone();
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    /// Chunk 6d ceiling: SKILL.md + the served schema stay under 6300 bytes
    /// combined (measured 5813 on 2026-10-03 — SKILL 2204 + schema 3609, after the duplicate-text trim).
    /// Raised from 6100 by owner decision when `find ids[]` joined `close
    /// ids[]`: batching the read and the write each saves an agent round per
    /// row, which outweighs ~120 bytes a session. The ≥40% cut was retargeted
    /// to ≥15% + a ceiling (PLAN-fael-durable-log §3). The ceiling sits just
    /// above the measure so any growth fails here, not only in the stats
    /// golden — raise it again only with a feature that earns it.
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
            total <= 6300,
            "constants {total} B exceed the 6300 B ceiling — trim, don't grow"
        );
        assert!(
            total * 100 <= BASELINE * 85,
            "constants {total} B kept less than 15% off the {BASELINE} B baseline"
        );
    }

    /// PLAN-fael-agent-ergonomics chunk 3 (experiment): the MCP surface shows
    /// the core only — hidden properties keep working, they are just not
    /// offered first. `cwd` is routing, not a filter, so it is ignored here.
    #[test]
    fn core_surface_is_what_agents_see() {
        let tools = super::tools();
        let props = |name: &str| -> BTreeSet<String> {
            tools
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == name)
                .unwrap()["inputSchema"]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .filter(|k| *k != "cwd")
                .cloned()
                .collect()
        };
        let set = |names: &[&str]| -> BTreeSet<String> {
            names.iter().map(ToString::to_string).collect()
        };
        assert_eq!(
            props("find"),
            set(&[
                "id", "ids", "text", "files", "key", "kind", "full", "all", "limit", "offset"
            ])
        );
        assert_eq!(
            props("add"),
            set(&[
                "kind",
                "text",
                "title",
                "files",
                "key",
                "supersedes",
                "rows",
                "to",
                "from",
                "revisit",
                "urgent"
            ])
        );
        assert_eq!(props("close"), set(&["id", "ids", "key", "text"]));
    }
}
