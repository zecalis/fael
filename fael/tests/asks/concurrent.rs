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
