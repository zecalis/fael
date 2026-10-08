//! PLAN-fael-sync chunk 2, commit 1: pin `fael stats` output before the
//! move to `fael-core::stats`. Any silent drift in the move fails here.
//!
//! The fixture uses `/work/real` (never a temp dir, never a real repo) so no
//! log join fires: every row status is `unknown`, every aggregate is local.

use super::fael_at;
use std::path::{Path, PathBuf};

/// Three usage rows with distinct event/client counts (deterministic order)
/// plus one torn line and one pull (`found`, no injection). Counts: read x2, edit x1, claude x2, codex x1,
// A x2, B x2.
fn golden_state() -> PathBuf {
    let state =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("stats-golden-{}", fael_core::ulid()));
    std::fs::create_dir_all(&state).unwrap();
    let rows = [
        r#"{"ts":"2026-09-26T00:00:00.000Z","repo":"/work/real","client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":["A"]}"#,
        r#"{"ts":"2026-09-26T00:01:00.000Z","repo":"/work/real","client":"claude","event":"read","bytes":20,"est_tokens":5,"ids":["A","B"],"session":"s1","said":[{"kind":"row","key":"A"},{"kind":"row","key":"B"},{"kind":"bodies"}]}"#,
        r#"{"ts":"2026-09-26T00:02:00.000Z","repo":"/work/real","client":"codex","event":"edit","bytes":30,"est_tokens":7,"ids":["B"]}"#,
        // a pull's outcome line: earns the bodies line, counts as no injection
        r#"{"ts":"2026-09-26T00:03:00.000Z","repo":"/work/real","client":"cli","event":"find","bytes":0,"est_tokens":0,"ids":[],"session":"s1","found":["B"],"q":{"id":"B"}}"#,
        "not json",
    ];
    std::fs::write(state.join("usage.jsonl"), rows.join("\n") + "\n").unwrap();
    state
}

/// The state path moves per run — pin everything after it, byte for byte.
fn normalize(out: &str, state: &Path) -> String {
    out.replace(
        &state.join("usage.jsonl").to_string_lossy().into_owned(),
        "<STATE>/usage.jsonl",
    )
}

// Constants below mirror SKILL.md + the MCP schema (asks.rs `constants`).
// If either source changes, update the numbers here deliberately.
// (PLAN-fael-agent-ergonomics chunk 3: schema 3609 → 2966, hidden find/add
// properties leave the served surface; they still work when called.)
const CONSTANTS: &str =
    "  constants per session: SKILL.md 2585 bytes (~650 est) + MCP schema 3043 bytes (~763 est)";

#[test]
fn stats_text_matches_golden() {
    let state = golden_state();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let (ok, out, err) = fael_at(&state, dir, &["stats"], "");
    assert!(ok, "{err}");
    assert_eq!(
        normalize(&out, &state),
        format!(
            "fael usage (<STATE>/usage.jsonl): 3 injections · 60 bytes · ~15 tokens into context\n  read: ×2 (~8 tokens)\n  edit: ×1 (~7 tokens)\n  client claude: ×2 (~8 tokens)\n  client codex: ×1 (~7 tokens)\n  asks: reject ×0 (0 bytes) · warning ×0 (0 bytes)\n  retired at touch: 0 of 2 pushed row(s) closed or superseded within a day of a push\n  acted on after said: row 0/2 (0%) · bodies 1/1 (100%)\n{CONSTANTS}\n"
        ),
        "{out}"
    );
}

#[test]
fn stats_rows_matches_golden() {
    let state = golden_state();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let (ok, out, err) = fael_at(&state, dir, &["stats", "--rows"], "");
    assert!(ok, "{err}");
    assert_eq!(
        normalize(&out, &state),
        format!(
            "fael usage (<STATE>/usage.jsonl): 3 injections · 60 bytes · ~15 tokens into context\n  read: ×2 (~8 tokens)\n  edit: ×1 (~7 tokens)\n  client claude: ×2 (~8 tokens)\n  client codex: ×1 (~7 tokens)\n  row A: pushed ×2 (unknown) · shown 1\n  row B: pushed ×2 (unknown) · shown 1 · pulled 1 (+0 fael-induced)\n  asks: reject ×0 (0 bytes) · warning ×0 (0 bytes)\n  retired at touch: 0 of 2 pushed row(s) closed or superseded within a day of a push\n  acted on after said: row 0/2 (0%) · bodies 1/1 (100%)\n{CONSTANTS}\n"
        ),
        "{out}"
    );
}

#[test]
fn stats_json_matches_golden_values() {
    // Maps (by_event/by_client) iterate a HashMap — compare values, not bytes.
    let state = golden_state();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let (ok, out, err) = fael_at(&state, dir, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v,
        serde_json::json!({
            "schema": 2, "outcomes_v": 1,
            "events": 3, "bytes": 60, "est_tokens": 15, "skipped_temp": 0,
            "by_event": {"read": {"events": 2, "est_tokens": 8}, "edit": {"events": 1, "est_tokens": 7}},
            "by_client": {"claude": {"events": 2, "est_tokens": 8}, "codex": {"events": 1, "est_tokens": 7}},
            "top_rows": [{"id": "A", "pushes": 2}, {"id": "B", "pushes": 2}],
            "asks": {"reject": {"events": 0, "bytes": 0}, "warning": {"events": 0, "bytes": 0}},
            "constants": {"skill_bytes": 2585, "skill_est": 650, "mcp_schema_bytes": 3043, "mcp_schema_est": 763},
            "rounds": {"rows_added": 0, "since": "2026-09-26"},
            "non_english_rows": {"rows": 0, "non_english": 0},
            "capture": {"reply_lines": 0, "reply_stored": 0, "reply_rejected": 0, "manual_adds": 0, "sessions_with_edits": 0, "sessions_with_edits_no_row": 0, "sessions_with_edits_gone": 0, "no_row_sessions": []},
            "retired": {"pushed": 2, "at_touch": 0},
            "context_loop": {"confirmed_repeats": 0, "edits_after_close": 0, "useful_shows": 0},
            "unused_rows": [],
            "incidents": {},
            "friction": {"calls": 0, "rejects": 0, "help": 0, "find_repeat": 0, "first_call_ok": 0, "reasons": {}, "by_command": {}},
            "file_verdict": {"changed": 0, "unchanged": 0, "no_verdict": 0, "no_fh": 0, "retire": {"changed": {"pairs": 0, "retired": 0}, "unchanged": {"pairs": 0, "retired": 0}}},
            "said": {"row": {"said": 2, "earned": 0}, "note": {"said": 0, "earned": 0}, "brief": {"said": 0, "earned": 0}, "ask": {"said": 0, "earned": 0}, "pointer": {"said": 0, "earned": 0}, "count": {"said": 0, "earned": 0}, "bodies": {"said": 1, "earned": 1}, "notice": {"said": 0, "earned": 0}, "cited": {"said": 0, "earned": 0}, "merge": {"said": 0, "earned": 0}},
            "value": {"in_context_at_edit": 0, "issues_closed": 0, "handoffs_picked_up": 0, "by_event": {}, "cross_agent": {"other_session": {"pushed": 0, "in_context_at_edit": 0}, "other_worktree": {"pushed": 0, "in_context_at_edit": 0}, "written_during_session": {"pushed": 0, "in_context_at_edit": 0}, "other_client": {"pushed": 0, "in_context_at_edit": 0}, "by_client": {}, "writer_unknown": 0, "same_file": {"sessions_seen": 0, "files": 0, "session_pairs": 0}}},
        }),
        "{out}"
    );
}

#[test]
fn stats_json_rows_matches_golden_values() {
    let state = golden_state();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let (ok, out, err) = fael_at(&state, dir, &["stats", "--json", "--rows"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["rows"],
        serde_json::json!([
            {"id": "A", "pushes": 2, "status": "unknown", "noise": false,
             "outcomes": {"shown": 1, "cut": {}, "cited": 0, "pulled": {"agent_initiated": 0, "fael_induced": 0}, "acted": 0, "retrieved_after_cut": 0, "missed_push": 0}},
            {"id": "B", "pushes": 2, "status": "unknown", "noise": false,
             "outcomes": {"shown": 1, "cut": {}, "cited": 0, "pulled": {"agent_initiated": 1, "fael_induced": 0}, "acted": 0, "retrieved_after_cut": 0, "missed_push": 0}},
        ]),
        "{out}"
    );
}

#[test]
fn stats_empty_matches_golden() {
    let state = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("stats-golden-empty-{}", fael_core::ulid()));
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("usage.jsonl"), "").unwrap();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let (ok, out, _) = fael_at(&state, dir, &["stats"], "");
    assert!(ok);
    assert_eq!(out, "fael: no usage recorded yet\n", "{out}");
    let (ok, out, _) = fael_at(&state, dir, &["stats", "--json"], "");
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["events"], 0, "{out}");
    assert_eq!(
        v["rounds"],
        serde_json::json!({"rows_added": 0, "since": ""}),
        "{out}"
    );
}
