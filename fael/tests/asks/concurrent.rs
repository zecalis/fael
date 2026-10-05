//! Parallel writers must never splice usage.jsonl mid-line ([01M3NWEE]):
//! sixteen threads hammer the reject path at once, then every line parses
//! and the row count matches — one torn line fails the test.

use super::{fael, repo};

#[test]
fn parallel_rejects_keep_every_line_parseable() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    const THREADS: usize = 16;
    const PER_THREAD: usize = 25;
    let d = std::sync::Arc::new(d);
    let gate = std::sync::Arc::new(std::sync::Barrier::new(THREADS));
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            let (d, gate) = (std::sync::Arc::clone(&d), std::sync::Arc::clone(&gate));
            s.spawn(move || {
                gate.wait();
                for _ in 0..PER_THREAD {
                    let (ok, _, _) = fael(&d, &["add", "bogus", "zz", "--files", "src/a.rs"], "");
                    assert!(!ok);
                }
            });
        }
    });
    let root = d.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let body = std::fs::read_to_string(root.join("state/usage.jsonl")).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), THREADS * PER_THREAD, "{body}");
    for l in &lines {
        assert!(
            serde_json::from_str::<serde_json::Value>(l).is_ok(),
            "torn line: {l}"
        );
    }
}

/// The month's first writers race to archive the live file (`usage_files`):
/// exactly one moves it, and no row — the old month's or a racer's — is lost.
#[test]
fn parallel_first_writes_of_a_month_archive_once_and_lose_no_row() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let state = d
        .ancestors()
        .find(|p| p.join(".git").exists())
        .unwrap()
        .join("state");
    std::fs::create_dir_all(&state).unwrap();
    let live = state.join("usage.jsonl");
    std::fs::write(
        &live,
        "{\"ts\":\"2020-03-10T00:00:00Z\",\"event\":\"old\"}\n",
    )
    .unwrap();
    let march = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_583_107_200);
    std::fs::File::options()
        .write(true)
        .open(&live)
        .unwrap()
        .set_modified(march)
        .unwrap();
    const THREADS: usize = 16;
    const PER_THREAD: usize = 5;
    let gate = std::sync::Barrier::new(THREADS);
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                gate.wait();
                for _ in 0..PER_THREAD {
                    let (ok, _, _) = fael(&d, &["add", "bogus", "zz", "--files", "src/a.rs"], "");
                    assert!(!ok);
                }
            });
        }
    });
    let archives: Vec<_> = std::fs::read_dir(state.join("usage"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(archives, ["2020-03.jsonl"]);
    let body = std::fs::read_to_string(state.join("usage/2020-03.jsonl")).unwrap()
        + &std::fs::read_to_string(&live).unwrap_or_default();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 1 + THREADS * PER_THREAD, "{body}");
    assert_eq!(lines.iter().filter(|l| l.contains("\"old\"")).count(), 1);
    for l in &lines {
        assert!(
            serde_json::from_str::<serde_json::Value>(l).is_ok(),
            "torn line: {l}"
        );
    }
}
