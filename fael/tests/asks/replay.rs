//! The chunk-3a baseline replay (PLAN-fael-durable-log §6.2a): one temp repo,
//! one writer, one branch, a fixed command sequence from the real cases — the
//! 5-note debt of `fix/cli-version-short-flag` (01M3HH57S), `Supersedes <id>`
//! in text without the flag (01M3HH57V), multi-adds on files with one / many /
//! no key — plus the reject and warning paths. Run BEFORE self-heal (3b–e);
//! the per-type counts below are the baseline the plan records under §3.
//! After 3b–e the same file re-runs: no ask type may rise, rejects must fall.

use super::{fael, json, repo, stats_json};

/// The replay repo: `src/a.rs` + `src/b.rs` (the debt files) and `src/c.rs`
/// (the keyed file), one writer, one branch throughout.
fn replay_repo() -> std::path::PathBuf {
    let d = repo();
    for f in ["src/a.rs", "src/b.rs", "src/c.rs"] {
        std::fs::write(d.join(f), format!("// {f}\n")).unwrap();
    }
    d
}

fn open_notes(d: &std::path::Path) -> usize {
    let (ok, out, err) = fael(d, &["find", "--kind", "note", "--json"], "");
    assert!(ok, "{err}");
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .count()
}

/// R1 — the 5-note debt, verbatim shape: same writer + branch, overlapping
/// files, one add per turn. Since 3b, adds 2–5 supersede the open note
/// themselves (`superseded` on stderr, no ask); before 3b all five stayed open.
#[test]
fn replay_debt_sequence_files_five_open_notes() {
    let d = replay_repo();
    for i in 1..=5 {
        let (ok, _, err) = fael(
            &d,
            &[
                "add",
                "note",
                &format!("progress {i} on the cli help"),
                "--files",
                "src/a.rs,src/b.rs",
            ],
            "",
        );
        assert!(ok, "add {i}: {err}");
        if i > 1 {
            assert!(err.contains("superseded"), "add {i}: {err}");
        }
    }
    assert_eq!(open_notes(&d), 1);
    let v = stats_json(&d);
    assert_eq!(v["asks"]["reject"]["events"], 0, "{v}");
    assert_eq!(v["asks"]["warning"]["events"], 0, "{v}");
    assert_eq!(v["asks"]["stop-block"]["events"], 0, "{v}");
}

/// R2 — `Supersedes <id>` in text, no flag (01M3HH57V): disjoint files, so
/// (b) stays out. Files today, the old row stays open; self-heal (3d) must
/// supersede it from the text, still asking nothing.
#[test]
fn replay_supersede_in_text_leaves_old_open() {
    let d = replay_repo();
    let (ok, out, err) = fael(
        &d,
        &["add", "note", "first pass", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let first = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("second pass. Supersedes {first}"),
            "--files",
            "src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert_eq!(open_notes(&d), 2);
    let v = stats_json(&d);
    assert_eq!(v["asks"]["reject"]["events"], 0, "{v}");
    assert_eq!(v["asks"]["warning"]["events"], 0, "{v}");
}

/// R3 — multi-adds on files with one / many / no key: all file keyless
/// today; self-heal (3e) must key the single-candidate one, asking nothing.
#[test]
fn replay_key_candidates_file_keyless() {
    let d = replay_repo();
    // keys share no parent and sit far apart — no similar-key warning fires
    for (f, key) in [
        ("src/c.rs", "auth:session"),
        ("src/b.rs", "zz:other"),
        ("src/b.rs", "cli:flags"),
    ] {
        let (ok, _, err) = fael(
            &d,
            &["add", "decision", "keyed", "--files", f, "--key", key],
            "",
        );
        assert!(ok, "{err}");
    }
    // one candidate (src/c.rs), two (src/b.rs), none (src/a.rs)
    for f in ["src/c.rs", "src/b.rs", "src/a.rs"] {
        let (ok, _, err) = fael(&d, &["add", "note", "follow-up", "--files", f], "");
        assert!(ok, "{err}");
    }
    let (ok, out, err) = fael(&d, &["find", "--files", "src/c.rs", "--json"], "");
    assert!(ok, "{err}");
    // today the follow-up files keyless: exactly one row carries the key
    let keyed = out
        .lines()
        .filter(|l| l.contains("\"key\":\"auth:session\""))
        .count();
    assert_eq!(keyed, 1, "{out}");
    let v = stats_json(&d);
    assert_eq!(v["asks"]["reject"]["events"], 0, "{v}");
    assert_eq!(v["asks"]["warning"]["events"], 0, "{v}");
}

/// R4 — same kind + key, same writer: (c) supersedes the old one, asking
/// nothing (the pre-3c baseline filed twice; the count is what changes, not
/// any ask type).
#[test]
fn replay_keyed_duplicate_supersedes() {
    let d = replay_repo();
    for i in 1..=2 {
        let (ok, _, err) = fael(
            &d,
            &[
                "add",
                "decision",
                &format!("stance {i}"),
                "--files",
                "src/c.rs",
                "--key",
                "auth:session",
            ],
            "",
        );
        assert!(ok, "add {i}: {err}");
        if i > 1 {
            assert!(err.contains("superseded"), "add {i}: {err}");
        }
    }
    let (ok, out, err) = fael(&d, &["find", "--key", "auth:session", "--json"], "");
    assert!(ok, "{err}");
    assert_eq!(out.lines().filter(|l| !l.trim().is_empty()).count(), 1);
}

/// R5 — `--supersedes` naming nothing: rejects today; self-heal (3d) must
/// rescue it when the text names exactly one open row.
#[test]
fn replay_unknown_supersedes_rejects() {
    let d = replay_repo();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "retry",
            "--files",
            "src/a.rs",
            "--supersedes",
            "nope-no-row",
        ],
        "",
    );
    assert!(!ok && err.contains("rejected: no row with id"), "{err}");
    assert_eq!(stats_json(&d)["asks"]["reject"]["events"], 1);
}

/// R6 — an ambiguous `--supersedes` prefix: rejects today with the
/// candidates; stays a reject after self-heal (genuinely ambiguous), only
/// the message may improve.
#[test]
fn replay_ambiguous_supersedes_rejects() {
    let d = replay_repo();
    for i in 1..=2 {
        let (ok, _, err) = fael(
            &d,
            &["add", "note", &format!("seed {i}"), "--files", "src/a.rs"],
            "",
        );
        assert!(ok, "{err}");
    }
    // every id starts `01`: ambiguous once two rows exist, deterministically
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "retry",
            "--files",
            "src/a.rs",
            "--supersedes",
            "01",
        ],
        "",
    );
    assert!(!ok && err.contains("rejected: id"), "{err}");
    assert_eq!(stats_json(&d)["asks"]["reject"]["events"], 1);
}

/// R7 — plain validation rejects: bad kind, no files, bad key. Self-heal
/// never touches these — the counts pin that.
#[test]
fn replay_validation_rejects() {
    let d = replay_repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "bogus", "zz", "--files", "src/a.rs"], "");
    assert!(!ok && err.contains("rejected:"), "{err}");
    let (ok, _, err) = fael(&d, &["add", "note", "nowhere"], "");
    assert!(!ok && err.contains("rejected: files is required"), "{err}");
    let (ok, _, err) = fael(
        &d,
        &[
            "add", "note", "bad key", "--files", "src/a.rs", "--key", "Bad Key",
        ],
        "",
    );
    assert!(!ok && err.contains("rejected: key"), "{err}");
    assert_eq!(stats_json(&d)["asks"]["reject"]["events"], 3);
}

/// R8 — a long untitled row: files with exactly one warning today; the
/// warning stays (self-heal files, it never silences).
#[test]
fn replay_long_untitled_warns_once() {
    let d = replay_repo();
    let text = vec!["word"; 70].join(" ");
    let (ok, _, err) = fael(&d, &["add", "note", &text, "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    assert!(err.contains("no title"), "{err}");
    let v = stats_json(&d);
    assert_eq!(v["asks"]["warning"]["events"], 1, "{v}");
    assert_eq!(v["asks"]["reject"]["events"], 0, "{v}");
}

/// R9 — one stop-block, then the row, then silence: pins stop-block counting
/// across the replay (blocking policy is unchanged by chunk 3).
#[test]
fn replay_stop_block_then_row_then_silence() {
    let d = replay_repo();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "old",
            "--files",
            "doc:seed",
            "--key",
            "test:seed",
        ],
        "",
    );
    assert!(ok, "{err}");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let t = d.join("t.jsonl");
    std::fs::write(&t, "").unwrap();
    let edit = format!(
        r#"{{"cwd":{},"session":{},"files":[{}]}}"#,
        json(&d),
        json(&t),
        json(&d.join("src/a.rs"))
    );
    let (ok, _, err) = fael(&d, &["hook", "edit"], &edit);
    assert!(ok, "{err}");
    let input = format!(r#"{{"cwd":{},"session":{}}}"#, json(&d), json(&t));
    let (ok, out, _) = fael(&d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":true"#), "{out}");
    let (ok, _, err) = fael(&d, &["add", "note", "covered", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    let (ok, out, _) = fael(&d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":false"#), "{out}");
    let v = stats_json(&d);
    assert_eq!(v["asks"]["stop-block"]["events"], 1, "{v}");
    assert_eq!(v["asks"]["reject"]["events"], 0, "{v}");
    assert_eq!(v["repeat_blocks"], 0, "{v}");
}
