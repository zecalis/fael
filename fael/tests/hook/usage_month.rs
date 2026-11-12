//! Usage stays bounded: the live `usage.jsonl` moves to `usage/<YYYY-MM>.jsonl`
//! at the first write of a new month, and a read with no `--since` covers the
//! live file plus the newest archive's `<month>.sum.json` for additive
//! counters — the archive raw is skipped once its summary exists.

use super::{fael_at, fael_at_env, repo};
use std::path::{Path, PathBuf};

fn line(ts: &str) -> String {
    format!(
        r#"{{"ts":"{ts}","repo":"/work/real","client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":["A"]}}"#
    ) + "\n"
}

fn state_dir(tag: &str) -> PathBuf {
    let s = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{tag}-{}", fael_core::ulid()));
    std::fs::create_dir_all(s.join("usage")).unwrap();
    s
}

fn events(state: &Path, extra: &[&str]) -> serde_json::Value {
    let args = [&["stats", "--json"][..], extra].concat();
    let (ok, out, err) = fael_at(state, state, &args, "");
    assert!(ok, "{out}{err}");
    serde_json::from_str::<serde_json::Value>(&out).unwrap()["events"].clone()
}

#[test]
fn a_read_with_no_since_opens_only_the_newest_archive_and_the_live_file() {
    let s = state_dir("usage-window");
    std::fs::write(s.join("usage/2020-01.jsonl"), line("2020-01-10T00:00:00Z")).unwrap();
    std::fs::write(s.join("usage/2020-02.jsonl"), line("2020-02-10T00:00:00Z")).unwrap();
    std::fs::write(s.join("usage.jsonl"), line("2020-03-10T00:00:00Z")).unwrap();
    assert_eq!(events(&s, &[]), 2);
    assert_eq!(events(&s, &["--since", "all"]), 3);
    assert_eq!(events(&s, &["--since", "2020-01-15"]), 2);
    let (ok, _, err) = fael_at(&s, &s, &["stats", "--since", "nope"], "");
    assert!(!ok && err.contains("or all"), "{err}");
}

#[test]
fn a_default_read_merges_the_closed_month_summary_for_additive_counters() {
    let s = state_dir("usage-merge");
    std::fs::write(s.join("usage/2020-02.jsonl"), line("2020-02-10T00:00:00Z")).unwrap();
    std::fs::write(s.join("usage.jsonl"), line("2020-03-10T00:00:00Z")).unwrap();
    // no summary yet: the default still reads the archive raw
    assert_eq!(events(&s, &[]), 2);
    // the run above backstopped the missing summary for the next run
    assert!(
        s.join("usage/2020-02.sum.json").is_file(),
        "missing closed summary is written for the next run"
    );
    // now the default skips the archive raw and merges its summary instead:
    // additive counters stay whole, the month rides listed in `summaries`
    let args = ["stats", "--json"];
    let (ok, out, err) = fael_at(&s, &s, &args, "");
    assert!(ok, "{out}{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["events"], 2);
    assert_eq!(v["by_event"]["read"]["events"], 2);
    assert_eq!(v["summaries"]["2020-02"]["events"], 1);
}

#[test]
fn the_first_write_of_a_new_month_archives_the_live_file_by_its_last_month() {
    let d = repo();
    // the state lives beside the repo under temp, like every other harness:
    // a temp repo with an outside state dir is scratch, and scratch runs
    // never write usage (issue 01M56WGH)
    let s = d.join("state");
    std::fs::create_dir_all(s.join("usage")).unwrap();
    let live = s.join("usage.jsonl");
    std::fs::write(&live, line("2020-03-10T00:00:00Z")).unwrap();
    let march = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_583_020_800 + 86_400);
    std::fs::File::options()
        .write(true)
        .open(&live)
        .unwrap()
        .set_modified(march)
        .unwrap();
    let d = repo();
    // a rejected add in an agent session is one usage row (its `call` line):
    // the write that finds March behind it
    let (ok, _, err) = fael_at_env(
        &s,
        &d,
        &["add", "decision", "x"],
        "",
        &[("FAEL_SESSION", "s")],
    );
    assert!(!ok && err.contains("rejected"), "{err}");
    let old = std::fs::read_to_string(s.join("usage/2020-03.jsonl")).unwrap();
    assert!(old.contains("2020-03-10"), "{old}");
    // rotation also writes the just-closed month's summary at once
    assert!(
        s.join("usage/2020-03.sum.json").is_file(),
        "rotation writes the summary with the archive"
    );
    let now = std::fs::read_to_string(&live).unwrap();
    assert!(
        !now.contains("2020-03-10") && now.contains("\"outcome\":\"reject\""),
        "{now}"
    );
}
