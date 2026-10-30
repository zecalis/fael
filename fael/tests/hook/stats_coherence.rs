//! c4 (time + stats coherence): stats derives ONLY from `usage.jsonl`
//! and a bare `--since YYYY-MM-DD` is local midnight — the CLI-level locks
//! beside the core unit tests in `fael-core`.
//!
use super::{fael, fael_at, fael_at_env, json, repo};

/// c4: stats derives ONLY from `usage.jsonl` — event/byte/token counts match
/// the raw usage lines exactly. A ledger row filed but never pushed adds
/// nothing, and kept-but-never-counted lines (`in-context`, `outcome`,
/// `call`, a pull's `found`, `silent-start`) plus a torn line contribute zero.
#[test]
fn stats_counts_come_from_usage_text_not_the_ledger() {
    let d = repo();
    let state = d.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // a ledger row exists but is never pushed: no usage line names it
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "unpushed choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let line = |ts: &str, event: &str, bytes: u64, est: u64, ids: &str| {
        format!(
            r#"{{"ts":"{ts}","repo":{},"client":"claude","event":"{event}","bytes":{bytes},"est_tokens":{est},"ids":[{ids}],"session":"s1"}}"#,
            json(&d)
        )
    };
    let body = [
        line("2026-09-26T00:00:00.000Z", "read", 10, 3, "\"A\""),
        line("2026-09-26T00:01:00.000Z", "edit", 20, 5, "\"A\""),
        line("2026-09-26T00:02:00.000Z", "search", 7, 2, ""),
        // kept, never counted: same bytes/tokens shape, must not move a total
        line("2026-09-26T00:03:00.000Z", "in-context", 9, 4, ""),
        line("2026-09-26T00:04:00.000Z", "outcome", 5, 1, ""),
        line("2026-09-26T00:05:00.000Z", "call", 6, 1, ""),
        line("2026-09-26T00:06:00.000Z", "silent-start", 8, 2, ""),
        format!(
            r#"{{"ts":"2026-09-26T00:07:00.000Z","repo":{},"client":"claude","event":"read","bytes":11,"est_tokens":3,"found":["A"]}}"#,
            json(&d)
        ),
        "torn line".to_string(),
    ]
    .join("\n")
        + "\n";
    std::fs::write(state.join("usage.jsonl"), &body).unwrap();
    let (ok, out, _) = fael_at(&state, &d, &["stats", "--json"], "");
    assert!(ok, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        v["events"] == 3 && v["bytes"] == 37 && v["est_tokens"] == 10,
        "{out}"
    );
    assert!(
        v["by_event"]["read"]["events"] == 1
            && v["by_event"]["edit"]["events"] == 1
            && v["by_event"]["search"]["events"] == 1,
        "{out}"
    );
    assert!(v["by_event"].get("in-context").is_none(), "{out}");
    assert!(v["by_event"].get("outcome").is_none(), "{out}");
    assert!(v["by_event"].get("call").is_none(), "{out}");
    assert!(v["by_event"].get("silent-start").is_none(), "{out}");
    assert!(v["skipped_temp"] == 0, "{out}");
    assert!(
        v["top_rows"] == serde_json::json!([{"id": "A", "pushes": 2}]),
        "{out}"
    );
}

/// c4: a bare `--since YYYY-MM-DD` is local midnight — with `FAEL_TZ_OFFSET=+07:00`
/// a 2026-09-29T18:00Z line (BKK 09-30 01:00) is inside the 09-30 window and a
/// 16:00Z line is out; `--json rounds` shows the window in Z and local.
#[test]
fn since_date_only_is_local_midnight() {
    let d = repo();
    let state = d.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let line = |ts: &str| {
        format!(
            r#"{{"ts":"{ts}","repo":{},"client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":[],"session":"s1"}}"#,
            json(&d)
        )
    };
    std::fs::write(
        state.join("usage.jsonl"),
        line("2026-09-29T18:00:00.000Z") + "\n" + &line("2026-09-29T16:00:00.000Z") + "\n",
    )
    .unwrap();
    let bkk = [("FAEL_TZ_OFFSET", "+07:00")];
    let (ok, out, _) = fael_at_env(
        &state,
        &d,
        &["stats", "--since", "2026-09-30", "--json"],
        "",
        &bkk,
    );
    assert!(ok, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["events"] == 1, "{out}");
    assert!(
        v["rounds"]["since_utc"] == "2026-09-29T18:00:00.000Z",
        "{out}"
    );
    assert!(
        v["rounds"]["since_local"] == "2026-09-30T01:00:00.000+07:00",
        "{out}"
    );
    assert!(v["rounds"]["tz_offset"] == "+07:00", "{out}");
    let (ok, out, _) = fael_at_env(&state, &d, &["stats", "--since", "2026-09-30"], "", &bkk);
    assert!(ok && out.contains("1 injections"), "{out}");
    // the same date at UTC midnight starts seven hours later: both lines out
    let utc = [("FAEL_TZ_OFFSET", "+00:00")];
    let (ok, out, _) = fael_at_env(
        &state,
        &d,
        &["stats", "--since", "2026-09-30", "--json"],
        "",
        &utc,
    );
    assert!(ok, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        v["events"] == 0 && v["rounds"]["tz_offset"] == "+00:00",
        "{out}"
    );
    assert!(v["rounds"]["since_utc"] == "", "{out}");
}
