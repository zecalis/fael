//! PLAN-fael-sync chunk 2, commit 1: pin `fael stats` output before the
//! move to `fael-core::stats`. Any silent drift in the move fails here.
//!
//! The fixture uses `/work/real` (never a temp dir, never a real repo) so no
//! log join fires: every row status is `unknown`, every aggregate is local.

use super::{fael_at, fael_at_env};
use std::path::{Path, PathBuf};

/// Three usage rows with distinct event/client counts (deterministic order)
/// plus one torn line and one pull (`found`, no injection). Counts: read x2, edit x1, claude x2, codex x1,
// A x2, B x2.
fn golden_state() -> PathBuf {
    let state =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("stats-golden-{}", fael_core::ulid()));
    std::fs::create_dir_all(&state).unwrap();
    let rows = [
        r#"{"ts":"2026-09-26T00:00:00.000Z","repo":"/work/real","client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":["A"],"ms":4}"#,
        r#"{"ts":"2026-09-26T00:01:00.000Z","repo":"/work/real","client":"claude","event":"read","bytes":20,"est_tokens":5,"ids":["A","B"],"ms":8,"session":"s1","said":[{"kind":"row","key":"A"},{"kind":"row","key":"B"},{"kind":"bodies"}]}"#,
        r#"{"ts":"2026-09-26T00:02:00.000Z","repo":"/work/real","client":"codex","event":"edit","bytes":30,"est_tokens":7,"ids":["B"],"ms":6}"#,
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
// Chunk tool (show|start|note|done|wait|after) joined MCP 2026-10-17:
// schema 3124 → 3739, one workflow without two screens.
// from: report (2026-10-22): skill 3308 → 3327, schema 3862 → 3875.
// Reject-with-try pass (2026-10-23): skill 3327 → 3333, one close example.
// z7 row language (2026-10-23): skill 3333 → 3298 — the add example upgraded
// the line-15 command in place (one topic + key) instead of duplicating it,
// the close example shortened, the discovery parenthetical trimmed.
// plan fael-secretary close (2026-10-31): skill 3298 → 3294 — the close example id is a `<id>` placeholder, not a phantom row id.
// Add batch schema (rows, force) 2026-10-22: schema 3739 → 3862, batch-only
// calls pass validation with no top-level required, one round per row.
// Close shape gate (force, issue-only) 2026-10-22: schema 3875 → 3874 — the
// new `force` paid by trimming the 4×-repeated `cwd` sentence.
// Chunk `review` (2026-10-22): schema 3850 → 3878.
// Chunk `pr` + `left` (2026-10-22): schema 3874 → 3850 — `done` through MCP
// alone, paid by trimming find.id/find.files/add.title/add.rows/cwd.
// Chunk `client` + `model` + `session` (2026-10-24, run-telemetry t1):
// schema 3878 → 4046 — `start` through MCP alone, for complete client/model.
// Chunk `with` + `as` (2026-10-24, issue 01M5PE238): schema 4046 → 4055 —
// joint `start` through MCP alone, paid by trimming chunk prop sentences
// (client/model/session/pr/left/review/uid/run) and the cwd sentence.
// Chunk `pr` takes a number too (2026-10-27): schema 4055 → 4067; `left` takes an array: → 4077, then "file as is," trimmed twice: 4053. `add.follows` (2026-11-01), paid by trimming find.id/ids and add.rows: 4064.
// Chunk `as` learns stack + cwd says the seat (2026-11-06, fael:01M6QCTD): schema 4064 → 4069 — `as` gains `|stack` (+6) and the chunk cwd names the seat default (+3), paid by two em-dashes to hyphens (-4).
// Chunk `because` joins the schema (2026-11-09): schema 4069 → 4068 — `start --force --because` through MCP alone, paid by trimming find/add/chunk prop sentences.
// Chunk `status` joins MCP (2026-11-11, fael:01M73BZA0E64Q4CX4E36DHCQBW): schema 4068 → 4065 — `status` (+7) paid by trimming force `takeover`→`take` (-4) and because `force receipt`→`receipt` (-6).
const CONSTANTS: &str =
    "  constants per session: SKILL.md 3294 bytes (~830 est) + MCP schema 4065 bytes (~1018 est)";

#[test]
fn stats_text_matches_golden() {
    let state = golden_state();
    // not under the repo: the lifecycle panel reads the repo `fael stats` runs in
    let dir = &std::env::temp_dir();
    let (ok, out, err) = fael_at(&state, dir, &["stats", "--all"], "");
    assert!(ok, "{err}");
    assert_eq!(
        normalize(&out, &state),
        format!(
            "fael usage (<STATE>/usage.jsonl): 3 injections · 60 bytes · ~15 tokens into context\n  read: ×2 (~8 tokens)\n  edit: ×1 (~7 tokens)\n  hook time ms p50/p95: edit 6/6 (n 1) · read 4/8 (n 2)\n  client claude: ×2 (~8 tokens)\n  client codex: ×1 (~7 tokens)\n  retired at touch: 0 of 2 pushed row(s) closed or superseded within a day of a push · bumped 0\n  acted on after said: row 0/2 (0%) · bodies 1/1 (100%)\n{CONSTANTS}\n"
        ),
        "{out}"
    );
}

#[test]
fn stats_rows_matches_golden() {
    let state = golden_state();
    // not under the repo: the lifecycle panel reads the repo `fael stats` runs in
    let dir = &std::env::temp_dir();
    let (ok, out, err) = fael_at(&state, dir, &["stats", "--all", "--rows"], "");
    assert!(ok, "{err}");
    assert_eq!(
        normalize(&out, &state),
        format!(
            "fael usage (<STATE>/usage.jsonl): 3 injections · 60 bytes · ~15 tokens into context\n  read: ×2 (~8 tokens)\n  edit: ×1 (~7 tokens)\n  hook time ms p50/p95: edit 6/6 (n 1) · read 4/8 (n 2)\n  client claude: ×2 (~8 tokens)\n  client codex: ×1 (~7 tokens)\n  row A: pushed ×2 (unknown) · shown 1\n  row B: pushed ×2 (unknown) · shown 1 · pulled 1 (+0 fael-induced)\n  retired at touch: 0 of 2 pushed row(s) closed or superseded within a day of a push · bumped 0\n  acted on after said: row 0/2 (0%) · bodies 1/1 (100%)\n{CONSTANTS}\n"
        ),
        "{out}"
    );
}

#[test]
fn stats_days_json_is_the_series_alone() {
    let state = golden_state();
    let dir = &std::env::temp_dir();
    let (ok, out, err) = fael_at(&state, dir, &["stats", "--days", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"days": [{"day": "2026-09-26", "events": 3, "est_tokens": 15}]})
    );
}

/// The zero `value` block, checked apart: one more nested map passes the
/// `json!` recursion limit, so it never stays inline in the golden test.
fn assert_value_zero(value: &serde_json::Value, out: &str) {
    assert_eq!(value["in_context_at_edit"], 0, "{out}");
    assert_eq!(value["issues_closed"], 0, "{out}");
    assert_eq!(value["handoffs_picked_up"], 0, "{out}");
    assert_eq!(value["handoffs_by_client"], serde_json::json!({}), "{out}");
    assert_eq!(value["handoffs_same_client"], 0, "{out}");
    assert_eq!(value["handoffs_writer_unknown"], 0, "{out}");
    assert_eq!(value["by_event"], serde_json::json!({}), "{out}");
    assert_eq!(
        value["cross_agent"],
        serde_json::json!({"other_session": {"pushed": 0, "in_context_at_edit": 0}, "other_worktree": {"pushed": 0, "in_context_at_edit": 0}, "written_during_session": {"pushed": 0, "in_context_at_edit": 0}, "other_client": {"pushed": 0, "in_context_at_edit": 0}, "by_client": {}, "writer_unknown": 0, "same_file": {"sessions_seen": 0, "files": 0, "session_pairs": 0}}),
        "{out}"
    );
    assert_eq!(
        value["decision_by_state"],
        serde_json::json!({"live": {"pushed": 0, "in_context_at_edit": 0}, "held": {"pushed": 0, "in_context_at_edit": 0}, "live_per_file": {"1": 0, "2": 0, "3": 0, ">3": 0}}),
        "{out}"
    );
}

#[test]
fn stats_json_matches_golden_values() {
    // Maps (by_event/by_client) iterate a HashMap — compare values, not bytes.
    let state = golden_state();
    // not under the repo: the lifecycle panel reads the repo `fael stats` runs in
    let dir = &std::env::temp_dir();
    // pin the viewer zone: `since_local`/`tz_offset` render it, so the pin
    // must not depend on the machine the test runs on
    let utc = [("FAEL_TZ_OFFSET", "+00:00")];
    let (ok, out, err) = fael_at_env(&state, dir, &["stats", "--json"], "", &utc);
    assert!(ok, "{err}");
    let mut v: serde_json::Value = serde_json::from_str(&out).unwrap();
    // `days` is checked apart: one more key passes the json! recursion limit
    let days = v.as_object_mut().unwrap().remove("days");
    assert_eq!(
        days,
        Some(serde_json::json!([{"day": "2026-09-26", "events": 3, "est_tokens": 15}]))
    );
    // also apart (same recursion limit): the one session s1 was handed A and B
    // (`shown` 2, none scored: no `in-context` line opens the window), edited
    // nothing, so both split tables are empty
    let sessions = v.as_object_mut().unwrap().remove("sessions");
    assert_eq!(
        sessions,
        Some(serde_json::json!([{
            "session": "s1", "repo": "/work/real", "client": "claude",
            "first": "2026-09-26T00:01:00.000Z", "last": "2026-09-26T00:03:00.000Z",
            "branch": null, "pr": null, "pr_checked": false, "edits": 0, "pushed_tokens": {"read": 5}, "tokens": null,
            "shown": 2, "scored": 0, "in_context": 0, "acted": 0,
            "calls": 0, "rejects": 0, "rows_filed": null, "in_run": false
        }]))
    );
    let group = serde_json::json!({"sessions": 0, "edits": 0, "calls": 0, "rejects": 0, "rows_filed": 0, "merged_pr": 0, "no_row": 0});
    let split = serde_json::json!({"with": group, "without": group});
    let splits = v.as_object_mut().unwrap().remove("session_splits");
    assert_eq!(
        splits,
        Some(serde_json::json!({"pushed": split, "in_run": split}))
    );
    // also apart (same recursion limit): no crew runs here, so both groups are empty
    let side = serde_json::json!({"runs": 0, "joined": 0, "done": 0, "finds": 0, "refinds": 0, "rejects": 0});
    let handoff = v.as_object_mut().unwrap().remove("handoff_runs");
    assert_eq!(
        handoff,
        Some(serde_json::json!({"with": side, "without": side}))
    );
    // also apart (same recursion limit): no repo log or gh here, so the
    // joins read empty and the total reads unknown, never an error
    let outcome = v.as_object_mut().unwrap().remove("outcome");
    assert_eq!(
        outcome,
        Some(
            serde_json::json!({"prs_merged": 0, "prs_created": 0, "prs_merged_unknown": true, "issues_filed": 0, "issues_closed": 0, "filed_to_close": {"median": 0, "p90": 0, "n": 0}, "joined_since": "", "prs_with_row": 0, "prs_without_row": 0, "window_since": "2026-09-26", "window_source": "usage"})
        )
    );
    // also apart (same recursion limit): no crew runs here, so the
    // attribution reads empty and no transcript tokens were measured
    let attribution = v.as_object_mut().unwrap().remove("models");
    assert_eq!(
        attribution,
        Some(
            serde_json::json!({"total": 0, "with_client_model": 0, "hint_only": 0, "none": 0, "with_cost": 0, "unknown_model_pct": 0.0, "unknown_cost_pct": 0.0, "by_model": {}})
        )
    );
    assert_eq!(v["context_by_client"], serde_json::json!({}));
    v.as_object_mut().unwrap().remove("context_by_client");
    assert_provenance_window(&mut v, &out);
    // also apart (same recursion limit): value carries the handoff split —
    // one more nested map passes the limit, so it is checked in pieces
    let value = v.as_object_mut().unwrap().remove("value").unwrap();
    assert_value_zero(&value, &out);
    assert_rest_zero(v, &out);
}

/// Provenance carries this run's recompute commands — the window is the
/// usage day, the range its day..today in the viewer zone.
fn assert_provenance_window(v: &mut serde_json::Value, _out: &str) {
    let provenance = v.as_object_mut().unwrap().remove("provenance");
    let p = provenance.as_ref().unwrap().as_object().unwrap();
    for k in [
        "outcome.prs_merged",
        "outcome.prs_with_row",
        "experience.fix_commits_linked",
        "context_loop.repeats",
    ] {
        assert!(p.contains_key(k), "{k} missing: {provenance:?}");
    }
    // FAEL_TZ_OFFSET=+00:00 above, so the range is the usage day..today
    assert!(
        p["outcome.prs_merged"]
            .as_str()
            .is_some_and(|s| s.contains("merged:2026-09-26..")),
        "{provenance:?}"
    );
}

/// Everything left after the apart-checked keys: `said` plus the top-level
/// shape. Its own `json!` call — one literal with every kind passes the
/// macro's recursion limit.
fn assert_rest_zero(v: serde_json::Value, out: &str) {
    // its own json! call: one literal with every kind passes the macro's recursion limit
    let said = serde_json::json!({"row": {"said": 2, "earned": 0, "by_event": {"read": {"said": 2, "earned": 0}}}, "note": {"said": 0, "earned": 0}, "brief": {"said": 0, "earned": 0}, "ask": {"said": 0, "earned": 0}, "pointer": {"said": 0, "earned": 0}, "bodies": {"said": 1, "earned": 1, "by_event": {"read": {"said": 1, "earned": 1}}}, "notice": {"said": 0, "earned": 0}, "cited": {"said": 0, "earned": 0}, "finding": {"said": 0, "earned": 0}, "fixed": {"said": 0, "earned": 0}, "fixcommit": {"said": 0, "earned": 0}, "held": {"said": 0, "earned": 0}, "overlap": {"said": 0, "earned": 0}, "peer": {"said": 0, "earned": 0}, "inbox": {"said": 0, "earned": 0}, "checkpoint": {"said": 0, "earned": 0}, "handoff": {"said": 0, "earned": 0}, "briefdrift": {"said": 0, "earned": 0}, "retired": {"said": 0, "earned": 0}, "chunkbrief": {"said": 0, "earned": 0}});
    assert_eq!(
        v,
        serde_json::json!({
            "schema": 7, "outcomes_v": 1,
            "events": 3, "bytes": 60, "est_tokens": 15, "skipped_temp": 0,
            "by_event": {"read": {"events": 2, "est_tokens": 8}, "edit": {"events": 1, "est_tokens": 7}},
            "by_client": {"claude": {"events": 2, "est_tokens": 8}, "codex": {"events": 1, "est_tokens": 7}},
            "top_rows": [{"id": "A", "pushes": 2}, {"id": "B", "pushes": 2}],
            "asks": {"warning": {"events": 0, "bytes": 0, "codes": {}}},
            "constants": {"skill_bytes": 3294, "skill_est": 830, "mcp_schema_bytes": 4065, "mcp_schema_est": 1018},
            "rounds": {"rows_added": 0, "since": "2026-09-26", "since_utc": "2026-09-26T00:00:00.000Z", "since_local": "2026-09-26T00:00:00.000+00:00", "tz_offset": "+00:00"},
            "non_english_rows": {"rows": 0, "non_english": 0},
            "lifecycle": {"decision": {"live": 0, "held": 0, "dormant": 0, "retired": 0}, "issue": {"live": 0, "held": 0, "dormant": 0, "retired": 0}, "note": {"live": 0, "held": 0, "dormant": 0, "retired": 0}},
            "capture": {"manual_adds": 0, "sessions_with_edits": 0, "sessions_with_edits_no_row": 0, "sessions_with_edits_gone": 0, "edit_session_issues": 0, "no_row_sessions": []},
            "retired": {"pushed": 2, "at_touch": 0, "bumped": 0},
            "context_loop": {"confirmed_repeats": 0, "edits_after_close": 0, "useful_shows": 0},
            "experience": {"fixed": 0, "fixed_from_review": 0, "closed_with_check": 0, "repeats_with_check": 0, "edits_after_close_with_check": 0, "fix_commits": 0, "fix_commits_linked": 0},
            "latency": {"edit": {"p50": 6, "p95": 6, "n": 1}, "read": {"p50": 4, "p95": 8, "n": 2}},
            "unused_rows": [],
            "flow": {"filed_to_close": {"median": 0, "p90": 0, "n": 0}, "start_to_done": {"median": 0, "p90": 0, "n": 0}, "done_to_merge": {"median": 0, "p90": 0, "n": 0}, "runs_with_pr": 0, "runs_total": 0},
            "throughput": [],
            "friction": {"calls": 0, "rejects": 0, "help": 0, "find_repeat": 0, "first_call_ok": 0, "reasons": {}, "by_command": {}, "by_client": {}, "shape_gate": 0, "shape_gate_rules": {}, "last_7d": {"calls": 0, "rejects": 0, "help": 0, "find_repeat": 0, "first_call_ok": 0, "reasons": {}}},
            "usage": {"since": "", "by_flag": []},
            "said": said,
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
    // a fresh repo of its own: the cwd repo's outcome reads even with no usage,
    // and target/tmp sits inside this checkout
    let dir = &super::repo();
    let (ok, out, _) = fael_at(&state, dir, &["stats"], "");
    assert!(ok);
    assert_eq!(out, "fael: no usage recorded yet\n", "{out}");
    let (ok, out, _) = fael_at_env(
        &state,
        dir,
        &["stats", "--json"],
        "",
        &[("FAEL_TZ_OFFSET", "+00:00")],
    );
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["events"], 0, "{out}");
    assert_eq!(
        v["rounds"],
        serde_json::json!({"rows_added": 0, "since": "", "since_utc": "", "since_local": "", "tz_offset": "+00:00"}),
        "{out}"
    );
}

/// A row pushed then bumped within a day counts under `bumped`, not
/// `retired at touch`.
#[test]
fn a_bumped_row_counts_under_bumped_not_retired() {
    let d = super::repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = super::fael(
        &d,
        &["add", "decision", "keep it", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let edit = format!(
        r#"{{"cwd":{},"session":"s1","files":["src/a.rs"]}}"#,
        super::json(&d)
    );
    let (ok, _, err) = super::fael(&d, &["hook", "edit"], &edit);
    assert!(ok, "{err}");
    let (ok, _, err) = super::fael(&d, &["bump", &id], "");
    assert!(ok, "{err}");
    let state = d.join("state");
    let (ok, out, err) = fael_at(&state, &d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["retired"]["pushed"], 1, "{out}");
    assert_eq!(v["retired"]["at_touch"], 0, "{out}");
    assert_eq!(v["retired"]["bumped"], 1, "{out}");
    let (ok, out, err) = fael_at(&state, &d, &["stats", "--all"], "");
    assert!(ok, "{err}");
    assert!(
        out.contains("retired at touch: 0 of 1 pushed row(s) closed or superseded within a day of a push · bumped 1"),
        "{out}"
    );
}
