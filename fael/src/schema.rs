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
    let cwd = str_("repo if not session cwd");
    let mut t = json!([
        {
            "name": "find",
            "description": "No args = the session brief; call it first only if no fael hook gave one.",
            "annotations": {"readOnlyHint": true},
            "inputSchema": {"type": "object", "properties": {
                "id": str_("id or prefix, pulls the body — a missing id rejects; text search: pass as text"),
                "ids": files("many ids, bodies in one call — a bad id fails alone"),
                "full": {"type": "boolean", "description": "bodies under titles"},
                "files": files("paths, dirs, globs, anchors — rows on any"),
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
            "description": "File what the next session needs: a decision and why, a bug (issue), or state (note). Add it with your next tool call or final edit, never as its own turn.",
            "inputSchema": {"type": "object", "properties": {
                "kind": str_("decision | issue | note, or a repo kind"),
                "text": str_("what happened and why, standalone, one topic (no ; or · list)"),
                "title": str_("≤15-word headline; required past 60 words/400 chars"),
                "files": {"type": "array", "items": {"type": "string"},
                    "description": "paths or anchors — omit for this session's edited files"},
                "rows": {"type": "array", "items": {"type": "object", "required": ["kind", "text"]},
                    "description": "batch rows; a bad row fails alone, the rest save"},
                "key": str_("optional colon key, e.g. auth:session"),
                "to": str_("who answers, e.g. ploy"),
                "from": str_("user (user's call) | report (relayed, unchecked)"),
                "revisit": str_("date YYYY-MM[-DD] or free text"),
                "urgent": {"type": "boolean", "description": "back of the urgent queue (issues)"},
                "supersedes": str_("id this replaces"),
                "force": {"type": "boolean", "description": "file as is, skip the shape gate"},
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
                "force": {"type": "boolean", "description": "file as is, skip the shape gate"},
                "text": str_(&format!("why; a fixed bug: {}", fael_core::stats::CLOSE_TEMPLATE)),
            }},
        },
        {
            "name": "chunk",
            "description": "Chunk lifecycle.",
            "inputSchema": {"type": "object", "required": ["verb", "uid"], "properties": {
                "verb": str_("show|start|note|done|wait|after"),
                "uid": str_("chunk uid"),
                "text": str_("note|handoff|why"),
                "other": str_("waits on"),
                "on": str_("owner|data"),
                "run": str_("run R"),
                "force": {"type": "boolean", "description": "take over"},
                "pr": str_("PR number for done"),
                "left": str_("leftover ids for done, or none"),
                "review": str_("done: clean|fixed|failed"),
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
    /// Chunk 6d ceiling: SKILL.md + the served schema stay under 6400 bytes
    /// combined (measured 5813 on 2026-10-03 — SKILL 2204 + schema 3609, after the duplicate-text trim).
    /// Raised from 6100 by owner decision when `find ids[]` joined `close
    /// ids[]`: batching the read and the write each saves an agent round per
    /// row, which outweighs ~120 bytes a session. The ≥40% cut was retargeted
    /// to ≥15% + a ceiling (PLAN-fael-durable-log §3). The ceiling sits just
    /// above the measure so any growth fails here, not only in the stats
    /// golden — raise it again only with a feature that earns it.
    ///
    /// Raised from 6300 to 6400 (measured 6394, 2026-10-10) when the close
    /// template gained its `don't <X> because <Y>` branch and the one-line
    /// question (PLAN-fael-capture-yield chunk 2): +94 B, once per session,
    /// until chunk 3 measures whether it earns it — if not, those lines come
    /// out and the ceiling goes back.
    ///
    /// Raised to 7100 (measured ~7014, 2026-10-17) when the `chunk`
    /// tool (show|start|note|done|wait|after) joined MCP: one workflow
    /// otherwise needs two screens (CLI + MCP). One tool with a `verb`
    /// keeps it to ~600 B, once per session.
    ///
    /// Raised to 7300 (measured 7170, 2026-10-22) when the `add` schema
    /// learned batch rows (`rows`, one round per extra row) and `force`.
    ///
    /// Close `force` (issue-only shape gate, 2026-10-22) paid by trimming the
    /// 4×-repeated `cwd` sentence, schema 3862 → 3861.
    ///
    /// Chunk `pr` + `left` (2026-10-22): `done` through MCP alone — an MCP
    /// `done` that named them was silently dropping them. Paid by trimming
    /// find.id/find.files/add.title/add.rows/cwd, schema 3874 → 3850.
    ///
    /// Chunk `review` (2026-10-22): `done --review` through MCP. Paid by
    /// trimming the chunk description (the `verb` field lists the verbs) and
    /// add.rows.
    ///
    /// Line endings are normalized first: `include_str!` reads the checkout,
    /// and a CRLF checkout (Windows) would add one byte per line without any
    /// content growing. The ceiling guards content, so measure canonical LF.
    ///
    /// English text, so −bytes = −tokens with no recount.
    const SKILL: &str = include_str!("../skill/SKILL.md");
    const BASELINE: usize = 7756;

    /// PLAN-fael-capture-yield chunk 2: one close template for every channel —
    /// the skill and the served `close` schema carry the core const verbatim.
    #[test]
    fn close_template_is_one_const() {
        let t = fael_core::stats::CLOSE_TEMPLATE;
        assert!(SKILL.contains(t), "SKILL.md lacks the close template");
        assert!(super::schema_json().contains(t), "schema lacks it");
        // label contract (01M4FD0Q): a template change lands with its doc
        let doc = include_str!("../../docs/format.md");
        assert!(doc.contains(t), "docs/format.md lacks it");
    }

    #[test]
    fn constants_stay_small() {
        let total = SKILL.replace("\r\n", "\n").len() + super::schema_json().len();
        assert!(
            total <= 7300,
            "constants {total} B exceed the 7300 B ceiling — trim, don't grow"
        );
        assert!(
            total * 100 <= BASELINE * 93,
            "constants {total} B kept less than 7% off the {BASELINE} B baseline"
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
                "urgent",
                "force"
            ])
        );
        assert_eq!(props("close"), set(&["id", "ids", "key", "text", "force"]));
        assert_eq!(
            props("chunk"),
            set(&[
                "verb", "uid", "text", "other", "on", "run", "force", "pr", "left", "review"
            ])
        );
    }

    /// KNOWN covers the schema: a new schema prop without a KNOWN entry would
    /// false-note a valid call, so the test fails instead of the agent's round.
    #[test]
    fn known_covers_the_schema() {
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
        let known =
            |ks: &[&str]| -> BTreeSet<String> { ks.iter().map(ToString::to_string).collect() };
        for (tool, k, h) in [
            ("find", crate::mcp::find::KNOWN, crate::mcp::find::HIDDEN),
            ("add", crate::mcp::add::KNOWN, crate::mcp::add::HIDDEN),
            ("close", crate::mcp::close::KNOWN, crate::mcp::close::HIDDEN),
            ("chunk", crate::mcp::chunk::KNOWN, crate::mcp::chunk::HIDDEN),
        ] {
            let missing: Vec<String> = props(tool).difference(&known(k)).cloned().collect();
            assert!(
                missing.is_empty(),
                "{tool}: schema props missing from KNOWN: {missing:?} — add them (or HIDDEN)"
            );
            let outside: Vec<&&str> = h.iter().filter(|x| !k.contains(x)).collect();
            assert!(
                outside.is_empty(),
                "{tool}: HIDDEN not in KNOWN: {outside:?}"
            );
        }
        // bump is unlisted (no schema surface) — its KNOWN is the whole set.
        assert!(!crate::mcp::bump::KNOWN.is_empty());
    }
}
