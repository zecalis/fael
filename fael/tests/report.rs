//! `fael report` and `fael stats --since`: the binary against a scratch state
//! dir — the page lands where `--out` says, counts only the window, and a bad
//! flag is rejected before anything is written.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// Two reads a day apart in a state dir that is itself scratch, so the
/// temp-repo filter keeps them.
fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-report-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("state")).unwrap();
    let line = |ts: &str, id: &str| {
        format!(
            "{{\"ts\":\"{ts}\",\"repo\":\"{}\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":3,\"ids\":[\"{id}\"]}}\n",
            d.join("repo").display()
        )
    };
    let text = line("2026-09-29T10:00:00.000Z", "OLD") + &line("2026-09-30T10:00:00.000Z", "NEW");
    std::fs::write(d.join("state/usage.jsonl"), text).unwrap();
    d
}

#[test]
fn report_writes_the_window_it_was_asked_for() {
    let d = dir();
    let out = d.join("r.html");
    let (ok, stdout, err) = fael(
        &d,
        &[
            "report",
            "--since",
            "2026-09-30",
            "--out",
            out.to_str().unwrap(),
        ],
    );
    assert!(ok, "{err}");
    assert!(stdout.contains("report written to"), "{stdout}");
    let page = std::fs::read_to_string(&out).unwrap();
    assert!(page.contains("fael find NEW"), "{page}");
    assert!(!page.contains("fael find OLD"), "{page}");
    assert!(page.contains("fael stats --json --since 2026-09-30"));
    assert!(!page.contains("<script"));
}

#[test]
fn stats_since_counts_only_the_window() {
    let d = dir();
    let (ok, out, err) = fael(&d, &["stats", "--json", "--since", "2026-09-30"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["events"], 1);
    assert_eq!(v["top_rows"][0]["id"], "NEW");
}

#[test]
fn bad_flags_are_rejected_before_writing() {
    let d = dir();
    let (ok, _, err) = fael(&d, &["report", "--since", "yesterday"]);
    assert!(!ok && err.contains("YYYY-MM-DD"), "{err}");
    let (ok, _, err) = fael(&d, &["report", "--json"]);
    assert!(!ok && err.contains("report takes no --json"), "{err}");
    assert!(!d.join("state/report.html").exists());
}
