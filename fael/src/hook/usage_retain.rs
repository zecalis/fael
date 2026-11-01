//! Class B2 of decision 01M5WC02: raw usage archives older than `KEEP_MONTHS`
//! are deleted, but only once `usage/<YYYY-MM>.sum.json` — that month's `Stats`,
//! computed by the same `aggregate` `fael stats` runs — is on disk. A summary is
//! kept forever. A read past the raw window gets the summaries per month: a
//! distinct count (sessions, unique rows) cannot be summed across months, so
//! they are never merged, only listed.

use super::{usage, usage_files as files};
use crate::core;
use serde_json::Value;
use std::path::Path;

/// Raw months kept before the current one.
const KEEP_MONTHS: i64 = 3;

fn cutoff(now_ms: u64) -> String {
    let m = files::month(now_ms);
    let (y, mo): (i64, i64) = (m[..4].parse().unwrap_or(0), m[5..7].parse().unwrap_or(1));
    let t = y * 12 + (mo - 1) - KEEP_MONTHS;
    format!("{:04}-{:02}", t.div_euclid(12), t.rem_euclid(12) + 1)
}

fn summary_path(month: &str) -> std::path::PathBuf {
    files::archive_dir().join(format!("{month}.sum.json"))
}

/// Write `month`'s summary (temp + rename) unless it is already there.
// ponytail: a summary holds what the month's usage rows say; repo logs, git
// and gh are not read (they describe the clone today, not that month), so the
// fields joined from them read empty.
fn summarise(raw: &Path, month: &str) -> Option<()> {
    let sum = summary_path(month);
    if sum.is_file() {
        return Some(());
    }
    let parsed = usage::parse_text(&std::fs::read_to_string(raw).ok()?);
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
pub(crate) fn retain() {
    let cut = cutoff(core::now_ms());
    for raw in files::all_archives() {
        let old = raw
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|m| *m < cut.as_str())
            .map(String::from);
        if let Some(m) = old
            && summarise(&raw, &m).is_some()
        {
            let _ = std::fs::remove_file(&raw);
        }
    }
}

/// The summaries of months from `since`'s on whose raw archive is gone,
/// oldest first. No `since` reads no summary.
pub(crate) fn summaries(since: Option<i64>) -> Vec<(String, Value)> {
    let Some(from) = since.map(|ms| files::month(ms.max(0) as u64)) else {
        return vec![];
    };
    let dir = files::archive_dir();
    let mut out: Vec<(String, Value)> = std::fs::read_dir(&dir)
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
        // SAFETY: nextest runs each test in its own process.
        unsafe { std::env::set_var("FAEL_STATE_DIR", &d) };
        retain();
        assert!(!a.join("2020-01.jsonl").exists() && a.join("2020-01.sum.json").is_file());
        assert!(a.join("2020-02.jsonl").exists(), "no summary, raw stays");
        assert!(a.join(format!("{recent}.jsonl")).exists());
        let s = summaries(Some(0));
        assert_eq!(
            s.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
            ["2020-01"]
        );
        assert_eq!(s[0].1["events"], 1);
        // no repo log, git or gh: the repo-joined rows read empty, not read from disk
        assert_eq!(s[0].1["capture"]["manual_adds"], 0);
        assert!(summaries(None).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
