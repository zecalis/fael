//! Class B2 of decision 01M5WC02: raw usage archives older than `KEEP_MONTHS`
//! are deleted, but only once `usage/<YYYY-MM>.sum.json` — that month's `Stats`,
//! computed by the same `aggregate` `fael stats` runs — is on disk. A summary is
//! kept forever. A read past the raw window gets the summaries per month: a
//! distinct count (sessions, unique rows) cannot be summed across months, so
//! they are never merged, only listed.

use super::{usage_files as files, usage_load};
use crate::core;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Raw months kept before the current one.
const KEEP_MONTHS: i64 = 3;

fn cutoff(now_ms: u64) -> String {
    let m = files::month(now_ms);
    let (y, mo): (i64, i64) = (m[..4].parse().unwrap_or(0), m[5..7].parse().unwrap_or(1));
    let t = y * 12 + (mo - 1) - KEEP_MONTHS;
    format!("{:04}-{:02}", t.div_euclid(12), t.rem_euclid(12) + 1)
}

fn summary_path_at(dir: &Path, month: &str) -> std::path::PathBuf {
    dir.join(format!("{month}.sum.json"))
}

/// Write `month`'s summary (temp + rename) unless it is already there.
/// `pub(crate)`: `rotate` writes the just-closed month's summary at rotation;
// retain_at below for the older months.
// ponytail: a summary holds what the month's usage rows say; repo logs, git
// and gh are not read (they describe the clone today, not that month), so the
// fields joined from them read empty.
pub(crate) fn summarise_at(dir: &Path, raw: &Path, month: &str) -> Option<()> {
    let sum = summary_path_at(dir, month);
    if sum.is_file() {
        return Some(());
    }
    let parsed = usage_load::parse_text(&std::fs::read_to_string(raw).ok()?);
    let s = core::stats::aggregate(
        &parsed,
        &Default::default(),
        &Default::default(),
        &[],
        &[],
        &core::Config::default(),
        super::asks::constants().into(),
        false,
        0, // UTC snapshot; the live read renders local at the CLI
        None,
        &[],
        None,
    );
    let tmp = sum.with_extension("tmp");
    let done = std::fs::write(&tmp, serde_json::to_vec(&s).ok()?)
        .and_then(|()| std::fs::rename(&tmp, &sum));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done.ok()
}

/// Delete raw archives older than the window, each only after its summary.
/// Explicit dir so unit tests never touch `FAEL_STATE_DIR`.
pub(crate) fn retain_at(dir: &Path) {
    let cut = cutoff(core::now_ms());
    for raw in files::all_archives_at(dir) {
        let old = raw
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|m| *m < cut.as_str())
            .map(String::from);
        if let Some(m) = old
            && summarise_at(dir, &raw, &m).is_some()
        {
            let _ = std::fs::remove_file(&raw);
        }
    }
}

/// The summaries of months from `since`'s on whose raw archive is gone,
/// oldest first. No `since` reads no summary.
pub(crate) fn summaries(since: Option<i64>) -> Vec<(String, Value)> {
    summaries_at(&files::archive_dir(), since)
}

/// The raw text a `stats` read covers plus the closed month to merge: with
/// no `--since`, live raw + the newest archive's summary when it exists
/// (the archive raw is skipped, so the month counts once); otherwise the
/// usual `read` (the archive raw, backstopped for the next run).
pub(crate) fn default_window(since: Option<i64>) -> (String, Option<(String, Value)>) {
    if since.is_some() {
        return (files::read(since), None);
    }
    if let Some(closed) = closed_summary_existing() {
        let live = std::fs::read_to_string(files::live()).unwrap_or_default();
        return (live, Some(closed));
    }
    (files::read(None), None)
}

/// The closed month's summary for the default window (no `--since`): the
/// newest archive's `<month>.sum.json` when it exists. No creation, no
/// fallback — the caller reads live raw only when this is `Some` (so the
/// month is not double-counted) and reads the archive raw when it is `None`.
fn closed_summary_existing() -> Option<(String, Value)> {
    closed_summary_existing_at(&files::archive_dir())
}

fn closed_summary_existing_at(dir: &Path) -> Option<(String, Value)> {
    let mut raws = files::all_archives_at(dir);
    raws.sort();
    let newest = raws.pop()?;
    let month = newest
        .file_stem()
        .and_then(|s| s.to_str())
        .map(String::from)?;
    let sum = dir.join(format!("{month}.sum.json"));
    if !sum.is_file() {
        return None;
    }
    let v: Value = serde_json::from_slice(&std::fs::read(sum).ok()?).ok()?;
    Some((month, v))
}

/// Best effort for the next run: write the newest archive's summary when it
/// is missing (rotation already writes it at close; this backstops archives
/// written by older binaries). Never merged in this run — the caller already
/// read the archive raw.
pub(crate) fn ensure_closed_summary() {
    let dir = files::archive_dir();
    let mut raws = files::all_archives_at(&dir);
    raws.sort();
    if let Some(newest) = raws.pop()
        && let Some(m) = newest
            .file_stem()
            .and_then(|s| s.to_str())
            .map(String::from)
    {
        let _ = summarise_at(&dir, &newest, &m);
    }
}

/// Add the closed month's summary to a live-only `Stats` where the stat is
/// additive: `events`, `bytes`, `est_tokens`, `by_event`, `by_client`.
/// Everything else keeps its live-raw value: stats that pair lines across
/// the month boundary (`repeat`/`context_loop`, `cross`, `handoff`) would
/// need the raw lines to join, and distinct counts (`sessions`, `top_rows`,
/// `by_id` pushes, `days`) cannot be summed — they stay live-only and the
/// month rides in `summaries` listed beside the totals (usage_retain rule).
pub(crate) fn merge_closed(stats: &mut core::stats::Stats, sum: &Value) {
    stats.events += sum["events"].as_u64().unwrap_or(0) as usize;
    stats.bytes += sum["bytes"].as_u64().unwrap_or(0) as usize;
    stats.est_tokens += sum["est_tokens"].as_u64().unwrap_or(0) as usize;
    merge_counts(&mut stats.by_event, &sum["by_event"]);
    merge_counts(&mut stats.by_client, &sum["by_client"]);
}

fn merge_counts(into: &mut BTreeMap<String, core::stats::Count>, from: &Value) {
    let Some(map) = from.as_object() else {
        return;
    };
    for (k, v) in map {
        let e = v["events"].as_u64().unwrap_or(0) as usize;
        let t = v["est_tokens"].as_u64().unwrap_or(0) as usize;
        match into.get_mut(k) {
            Some(c) => {
                c.events += e;
                c.est_tokens += t;
            }
            None => {
                into.insert(
                    k.clone(),
                    core::stats::Count {
                        events: e,
                        est_tokens: t,
                    },
                );
            }
        }
    }
}

/// Explicit dir (see `retain_at`).
pub(crate) fn summaries_at(dir: &Path, since: Option<i64>) -> Vec<(String, Value)> {
    let Some(from) = since.map(|ms| files::month(ms.max(0) as u64)) else {
        return vec![];
    };
    let mut out: Vec<(String, Value)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let m = name.strip_suffix(".sum.json")?.to_string();
            if m < from || dir.join(format!("{m}.jsonl")).exists() {
                return None;
            }
            let v = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            Some((m, v))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `--json`: the window's `Stats` plus `summaries` (month → that month's
/// `Stats`) when the window reaches past the raw files.
pub(crate) fn json(
    stats: &core::stats::Stats,
    sums: Vec<(String, Value)>,
) -> Result<String, String> {
    let mut v = serde_json::to_value(stats).map_err(|e| e.to_string())?;
    if !sums.is_empty() {
        v["summaries"] = Value::Object(sums.into_iter().collect());
    }
    serde_json::to_string(&v).map_err(|e| e.to_string())
}

/// One line per summarised month: only additive counters (events, tokens,
/// events by kind).
pub(crate) fn print(sums: &[(String, Value)]) {
    for (m, s) in sums {
        let n = |v: &Value| v.as_u64().unwrap_or(0);
        let by: Vec<String> = s["by_event"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, c)| format!("{k} ×{}", n(&c["events"])))
            .collect();
        println!(
            "  {m} (summary): {} events · ~{} tokens · {}",
            n(&s["events"]),
            n(&s["est_tokens"]),
            by.join(" · ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_steps_back_across_a_year() {
        assert_eq!(cutoff(1_790_000_000_000), "2026-06"); // 2026-09
        assert_eq!(cutoff(1_768_000_000_000), "2025-10"); // 2026-01
    }

    #[test]
    fn raw_goes_only_after_its_summary_is_written() {
        let d = std::env::temp_dir().join(format!("fael-retain-{}", core::ulid()));
        let a = d.join("usage");
        std::fs::create_dir_all(&a).unwrap();
        let row = r#"{"ts":"2020-01-05T00:00:00.000Z","event":"push","repo":"/nonexistent","bytes":10,"est_tokens":5}"#;
        for m in ["2020-01", "2020-02"] {
            std::fs::write(a.join(format!("{m}.jsonl")), format!("{row}\n")).unwrap();
        }
        let recent = files::month(core::now_ms());
        std::fs::write(a.join(format!("{recent}.jsonl")), format!("{row}\n")).unwrap();
        // 2020-02's summary cannot be written: its path is a directory
        std::fs::create_dir_all(a.join("2020-02.sum.json")).unwrap();
        retain_at(&a);
        assert!(!a.join("2020-01.jsonl").exists() && a.join("2020-01.sum.json").is_file());
        assert!(a.join("2020-02.jsonl").exists(), "no summary, raw stays");
        assert!(a.join(format!("{recent}.jsonl")).exists());
        let s = summaries_at(&a, Some(0));
        assert_eq!(
            s.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
            ["2020-01"]
        );
        assert_eq!(s[0].1["events"], 1);
        // no repo log, git or gh: the repo-joined rows read empty, not read from disk
        assert_eq!(s[0].1["capture"]["manual_adds"], 0);
        assert!(summaries_at(&a, None).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn closed_summary_is_the_newest_archive_with_a_summary_file() {
        let d = std::env::temp_dir().join(format!("fael-closed-{}", core::ulid()));
        let a = d.join("usage");
        std::fs::create_dir_all(&a).unwrap();
        let row = r#"{"ts":"2020-01-05T00:00:00.000Z","event":"read","repo":"/nonexistent","bytes":10,"est_tokens":5,"ids":[]}"#;
        std::fs::write(a.join("2020-01.jsonl"), format!("{row}\n")).unwrap();
        // no summary yet: nothing to merge, so the caller keeps the raw
        assert!(closed_summary_existing_at(&a).is_none());
        summarise_at(&a, &a.join("2020-01.jsonl"), "2020-01").unwrap();
        let (m, v) = closed_summary_existing_at(&a).unwrap();
        assert_eq!(m.as_str(), "2020-01");
        assert_eq!(v["events"], 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn merge_closed_adds_only_the_additive_counters() {
        let d = std::env::temp_dir().join(format!("fael-merge-{}", core::ulid()));
        let a = d.join("usage");
        std::fs::create_dir_all(&a).unwrap();
        let row = |ev: &str| {
            format!(
                "{{\"ts\":\"2020-02-05T00:00:00.000Z\",\"event\":\"{ev}\",\"repo\":\"/nonexistent\",\"client\":\"claude\",\"bytes\":10,\"est_tokens\":5,\"ids\":[]}}\n"
            )
        };
        std::fs::write(a.join("2020-01.jsonl"), row("read")).unwrap();
        summarise_at(&a, &a.join("2020-01.jsonl"), "2020-01").unwrap();
        let (_, sum) = closed_summary_existing_at(&a).unwrap();
        let parsed = usage_load::parse_text(&row("edit"));
        let mut s = core::stats::aggregate(
            &parsed,
            &Default::default(),
            &Default::default(),
            &[],
            &[],
            &core::Config::default(),
            (0, 0, 0, 0).into(),
            false,
            0,
            None,
            &[],
            None,
        );
        assert_eq!((s.events, s.bytes, s.est_tokens), (1, 10, 5));
        merge_closed(&mut s, &sum);
        assert_eq!((s.events, s.bytes, s.est_tokens), (2, 20, 10));
        assert_eq!(s.by_event["read"].events, 1);
        assert_eq!(s.by_event["edit"].events, 1);
        assert_eq!(s.by_client["claude"].events, 2);
        // distinct counts stay live-only: the archived month holds no session
        // join and the live parse holds the one live row's session stem
        assert!(s.sessions.len() <= 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
