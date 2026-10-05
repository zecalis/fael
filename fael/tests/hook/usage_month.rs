//! Usage stays bounded: the live `usage.jsonl` moves to `usage/<YYYY-MM>.jsonl`
//! at the first write of a new month, and a read with no `--since` opens only
//! the newest archive and the live file.

use super::{fael_at, repo};
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
fn the_first_write_of_a_new_month_archives_the_live_file_by_its_last_month() {
    let s = state_dir("usage-rotate");
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
    // a rejected add is one usage row: the write that finds March behind it
    let (ok, _, err) = fael_at(&s, &d, &["add", "decision", "x"], "");
    assert!(!ok && err.contains("rejected"), "{err}");
    let old = std::fs::read_to_string(s.join("usage/2020-03.jsonl")).unwrap();
    assert!(old.contains("2020-03-10"), "{old}");
    let now = std::fs::read_to_string(&live).unwrap();
    assert!(
        !now.contains("2020-03-10") && now.contains("\"ask\":\"reject\""),
        "{now}"
    );
}
