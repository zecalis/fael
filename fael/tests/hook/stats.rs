//! Usage accounting: temp-dir repos are skipped unless the state dir is
//! scratch too.

use super::{fael, fael_at, json, repo};
use std::path::Path;

#[test]
fn stats_skips_temp_repos_unless_state_is_scratch_too() {
    // a real state dir (outside the OS temp dir) drops usage from temp-dir
    // benchmark repos (01M3CRR6A)
    let state = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("stats-{}", fael_core::ulid()));
    std::fs::create_dir_all(&state).unwrap();
    let tmp_repo = std::env::temp_dir().join("faelbench.x");
    let line = |repo: &Path| {
        format!(
            r#"{{"ts":"2026-09-26T00:00:00.000Z","repo":{},"client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":["A"]}}"#,
            json(repo)
        )
    };
    std::fs::write(
        state.join("usage.jsonl"),
        format!("{}\n{}\n", line(&tmp_repo), line(Path::new("/work/real"))),
    )
    .unwrap();
    let (ok, out, _) = fael_at(&state, &state, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(ok && v["events"] == 1 && v["skipped_temp"] == 1, "{out}");
}

#[test]
fn push_usage_counts_only_rendered_rows() {
    // chunk 2: the budget cuts the push, so usage must count only the ids
    // that were actually said — never the rows render cut off
    let d = repo();
    // a tiny push budget so render cuts: usage must count only the ids
    // that were actually said — never the rows render cut off
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_tokens = 30\n").unwrap();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    for i in 0..4 {
        let (ok, _, err) = fael(
            &d,
            &[
                "add",
                "decision",
                &format!("choice {i}"),
                "--files",
                "src/a.rs",
            ],
            "",
        );
        assert!(ok, "{err}");
    }
    // a same-dir neighbour: a read must not push it, an edit must
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "neighbour choice", "--files", "src/b.rs"],
        "",
    );
    assert!(ok, "{err}");
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "read"], &input);
    // chunk 1 (push-focus): the budget cut surfaces as the omitted line with
    // the exact next call, not render's budget line
    assert!(ok && out.contains("more about this file"), "{out}");
    let reply: serde_json::Value = serde_json::from_str(&out).unwrap();
    let context = reply["context"].as_str().unwrap();
    let rendered = context.lines().filter(|l| l.starts_with("- [")).count();
    assert!(rendered > 0 && rendered < 4, "{context}");
    assert!(!context.contains("neighbour choice"), "{context}");
    let usage = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap();
    let v: serde_json::Value = serde_json::from_str(usage.lines().last().unwrap()).unwrap();
    assert_eq!(v["ids"].as_array().unwrap().len(), rendered, "{v}");
    // the full budget back: an edit still counts the same-dir neighbour in
    // the omitted line (push-focus chunk 1: same-dir decisions are
    // Background — counted, never rendered)
    std::fs::write(
        d.join(".fael/config.toml"),
        "[budget]\npush_tokens = 10000\n",
    )
    .unwrap();
    let (ok, out, _) = fael(&d, &["hook", "edit"], &input);
    assert!(
        ok && out.contains("… +1 more about this file — fael find --files src/a.rs"),
        "{out}"
    );
}

#[test]
fn stats_rows_shows_pushes_status_and_noise() {
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
    let open = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(
        &d,
        &["add", "decision", "done choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let shut = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(&d, &["close", &shut, "done"], "");
    assert!(ok, "{err}");
    let line = |id: &str| {
        format!(
            r#"{{"ts":"2026-09-26T00:00:00.000Z","repo":{},"client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":["{id}"]}}"#,
            json(&d)
        )
    };
    let mut body = String::new();
    for _ in 0..12 {
        body.push_str(&line(&open));
        body.push('\n');
    }
    for _ in 0..3 {
        body.push_str(&line(&shut));
        body.push('\n');
    }
    body.push_str(&line("AAAAAAAAAAAAAAAAAAAAAAAAAA"));
    body.push('\n');
    std::fs::write(state.join("usage.jsonl"), body).unwrap();
    let (ok, out, _) = fael_at(&state, &d, &["stats", "--rows"], "");
    assert!(ok, "{out}");
    assert!(
        out.contains(&format!("row {open}: pushed ×12 (open) noise?")),
        "{out}"
    );
    // the --rows section (not the top-10 above it): closed, no noise flag
    let shut_line = out
        .lines()
        .find(|l| l.contains(&shut) && l.contains("(closed)"))
        .unwrap_or("");
    assert!(!shut_line.contains("noise?"), "{out}");
    assert!(out.contains("pushed ×1 (unknown)"), "{out}");
    let (ok, out, _) = fael_at(&state, &d, &["stats", "--json", "--rows"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(ok, "{out}");
    let first = &v["rows"][0];
    assert!(
        first["id"] == open
            && first["pushes"] == 12
            && first["status"] == "open"
            && first["noise"] == true,
        "{out}"
    );
}

#[test]
fn stats_rows_sees_local_store_journal_rows() {
    // store = "local" keeps every row in the journal, never the tree log —
    // stats must read the same union the hooks do, or every status is unknown
    let d = repo();
    let state = d.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &["add", "decision", "journal only", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    assert!(
        !d.join(".fael/log").exists(),
        "local mode must not write the tree log"
    );
    let line = format!(
        r#"{{"ts":"2026-09-26T00:00:00.000Z","repo":{},"client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":["{id}"]}}"#,
        json(&d)
    );
    std::fs::write(state.join("usage.jsonl"), format!("{line}\n")).unwrap();
    let (ok, out, _) = fael_at(&state, &d, &["stats", "--rows"], "");
    assert!(ok, "{out}");
    assert!(
        out.contains(&format!("row {id}: pushed ×1 (open)")),
        "{out}"
    );
}
