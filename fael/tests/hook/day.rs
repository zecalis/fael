//! `fael stats --day` (PLAN-fael-sync chunk 3): the CLI prints the same
//! `DayView` the desktop popover reads, its numbers match `fael stats` on a
//! one-day log, and an empty state dir is zeros + exit 0.

use super::{fael, fael_at, fael_at_env, json, repo};
use std::path::Path;

fn day_json(state: &Path, dir: &Path) -> (bool, serde_json::Value, String) {
    let (ok, out, err) = fael_at_env(
        state,
        dir,
        &["stats", "--day", "--json"],
        "",
        &[("FAEL_TZ_OFFSET", "Z")],
    );
    let v = serde_json::from_str(&out).unwrap_or_default();
    (ok, v, out + &err)
}

/// One day of usage: every row is today (UTC), so the day view's tokens
/// must equal `fael stats`' totals — one day, one truth. This is the share
/// check of PLAN-fael-sync §3: same inputs, same numbers.
#[test]
fn day_json_shares_stats_numbers() {
    let state =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("stats-day-{}", fael_core::ulid()));
    std::fs::create_dir_all(&state).unwrap();
    let today = fael_core::now_ms();
    let day = &fael_core::rfc3339(today as u64)[..10];
    let at = |mins: u64, ids: &str, real: bool| {
        let ts = format!("{day}T10:{:02}:00.000Z", mins);
        let mut l = format!(
            r#"{{"ts":"{ts}","repo":"/work/real","client":"claude","event":"read","bytes":10,"est_tokens":25,"ids":[{ids}],"session":"s1""#
        );
        if real {
            l.push_str(
                r#","real_tokens":{"input_tokens":1000,"cache_creation_input_tokens":2000,"cache_read_input_tokens":3000,"output_tokens":50}"#,
            );
        }
        l.push_str("}\n");
        l
    };
    std::fs::write(
        state.join("usage.jsonl"),
        at(0, r#""A""#, false) + &at(1, r#""A","B""#, true) + &at(2, r#""C""#, true),
    )
    .unwrap();
    let (_, stats, _) = fael_at(&state, &state, &["stats", "--json"], "");
    let stats: serde_json::Value = serde_json::from_str(&stats).unwrap();
    let (ok, day_v, txt) = day_json(&state, &state);
    assert!(ok, "{txt}");
    let all = &day_v["all"];
    assert_eq!(day_v["schema"], 1, "{day_v}");
    assert_eq!(day_v["day"].as_str(), Some(day), "{day_v}");
    assert_eq!(day_v["tz_offset"].as_str(), Some("+00:00"), "{day_v}");
    // fael's tokens of the day == fael stats' total on this one-day log
    assert_eq!(
        all["context"]["fael_tokens"], stats["est_tokens"],
        "{day_v}"
    );
    // session side: in + cache-create + cache-read of both measured rows
    assert_eq!(all["context"]["session_tokens"], 12_000, "{day_v}");
    let share = all["context"]["share"].as_f64().unwrap();
    assert_eq!(
        share,
        stats["est_tokens"].as_f64().unwrap() / 12_000.0,
        "{day_v}"
    );
    // three pushes today, newest last in `last`
    assert_eq!(all["delivered"]["rows"], 3, "{day_v}");
    assert_eq!(
        all["delivered"]["last"].as_array().unwrap().len(),
        3,
        "{day_v}"
    );
    assert_eq!(
        day_v["repos"][0]["repo"].as_str(),
        Some("/work/real"),
        "{day_v}"
    );
    assert_eq!(
        all["timeline"]["delivered"].as_array().unwrap().len(),
        96,
        "{day_v}"
    );
    // `cwd` sits inside this worktree, so the CLI resolves a writer and
    // `for_you` is an object (no row routes to anyone: rows = 0) — with no
    // repo above it, it would be null (core's empty-view test pins that).
    assert_eq!(all["for_you"]["rows"], 0, "{day_v}");
}

/// The day view reads a real repo's log: the pushed row's title and file
/// resolve, and the repo is named per entry plus summed into `all`.
#[test]
fn day_view_resolves_rows_from_the_repo_log() {
    let d = repo();
    let state = d.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &["add", "decision", "kept choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    let day = &fael_core::rfc3339(fael_core::now_ms() as u64)[..10];
    let line = format!(
        r#"{{"ts":"{day}T10:00:00.000Z","repo":{},"client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":["{id}"]}}"#,
        json(&d)
    );
    std::fs::write(state.join("usage.jsonl"), format!("{line}\n")).unwrap();
    let (ok, v, txt) = day_json(&state, &d);
    assert!(ok, "{txt}");
    let last = &v["all"]["delivered"]["last"][0];
    assert_eq!(last["id"].as_str(), Some(id.as_str()), "{v}");
    assert_eq!(last["title"].as_str(), Some("kept choice"), "{v}");
    assert_eq!(last["file"].as_str(), Some("src/a.rs"), "{v}");
    assert_eq!(v["repos"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v["all"]["delivered"]["rows"], 1, "{v}");
}

/// No state dir yet (nothing recorded): `DayView` is zeros and `repos` is
/// empty — exit 0, never an error (PLAN-fael-sync §7).
#[test]
fn day_view_without_state_dir_is_zeros() {
    let state = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("stats-day-empty-{}", fael_core::ulid()));
    std::fs::create_dir_all(&state).unwrap();
    let (ok, v, txt) = day_json(&state, &state);
    assert!(ok, "{txt}");
    assert_eq!(v["schema"], 1, "{txt}");
    assert_eq!(v["all"]["delivered"]["rows"], 0, "{txt}");
    assert_eq!(v["all"]["context"]["fael_tokens"], 0, "{txt}");
    assert!(v["all"]["context"]["share"].is_null(), "{txt}");
    assert!(v["repos"].as_array().is_some_and(|r| r.is_empty()), "{txt}");
}

/// The human text is smoke-checked: `--day` without `--json` prints the
/// day header, and `FAEL_TZ_OFFSET` reaches the output verbatim.
#[test]
fn day_text_prints_and_honours_tz_offset() {
    let state = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("stats-day-text-{}", fael_core::ulid()));
    std::fs::create_dir_all(&state).unwrap();
    let day = &fael_core::rfc3339(fael_core::now_ms() as u64)[..10];
    std::fs::write(
        state.join("usage.jsonl"),
        format!(
            r#"{{"ts":"{day}T10:00:00.000Z","repo":"/work/real","client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":["A"]}}"#
        ),
    )
    .unwrap();
    let (ok, out, err) = fael_at_env(
        &state,
        &state,
        &["stats", "--day"],
        "",
        &[("FAEL_TZ_OFFSET", "+07:00")],
    );
    assert!(ok, "{err}");
    assert!(
        out.starts_with(&format!("fael today ({day} +07:00):")),
        "{out}"
    );
    assert!(out.contains("health:"), "{out}");
}
