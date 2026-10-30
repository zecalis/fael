//! c7 preset acceptance matrix (plan fael-secretary): a fresh clone with
//! `fael install --preset multi-agent` passes the 5-workflow smoke (kickoff,
//! add, close, supersede, doctor) with final-ledger assertions, duplicate and
//! retry idempotence, cross-session pushes (F-XSESSION under the preset), and
//! the chunk + plan-lint legs (resume returns the live R; done lands in
//! review) — all with zero config edits.
//!
//! State dir: every spawn below goes through `preset::fael` / `preset::hook`,
//! which point FAEL_STATE_DIR at the test's scratch home (never the real one).

use super::preset::{fael, hook, id_of, scratch};
use std::path::{Path, PathBuf};

/// Add A, duplicate-add it, supersede A -> B, close B — every repeat names
/// the existing row. Returns the (A, B) ids for the ledger assertions.
fn add_close_cycle(home: &Path, repo: &Path) -> (String, String) {
    let add_v1 = [
        "add",
        "issue",
        "smoke hot path",
        "--title",
        "smoke",
        "--files",
        "src/a.rs",
    ];
    let (ok, out, err) = fael(home, repo, &add_v1);
    assert!(ok, "{err}");
    let a = id_of(&out).to_string();
    let (ok, out2, _) = fael(home, repo, &add_v1);
    assert!(ok, "{out2}");
    assert_eq!(id_of(&out2), a.as_str(), "retry names the existing row");
    let add_v2 = [
        "add",
        "issue",
        "smoke hot path v2",
        "--title",
        "smoke v2",
        "--files",
        "src/a.rs",
        "--supersedes",
        &a,
    ];
    let (ok, out, err) = fael(home, repo, &add_v2);
    assert!(ok, "{err}");
    let b = id_of(&out).to_string();
    let (ok, out2, _) = fael(home, repo, &add_v2);
    assert!(ok, "{out2}");
    assert_eq!(
        id_of(&out2),
        b.as_str(),
        "supersede retry names the survivor"
    );
    let close = [
        "close",
        &b,
        "smoke broke → smoke fixed; guard `src/a_test.rs`",
    ];
    let (ok, out, err) = fael(home, repo, &close);
    assert!(ok, "{err}");
    let c = id_of(&out).to_string();
    let (ok, out2, err2) = fael(home, repo, &close);
    assert!(ok, "{err2}");
    assert_eq!(
        id_of(&out2),
        c.as_str(),
        "close retry names the existing close"
    );
    (a, b)
}

/// Final ledger, not just exit 0: A superseded by B, B closed, edge on B.
fn assert_ledger(home: &Path, repo: &Path, a: &str, b: &str) {
    let (ok, out, _) = fael(home, repo, &["find", a]);
    assert!(ok, "{out}");
    assert!(
        out.contains("superseded") && out.contains(&b[..8]),
        "A names B as its successor: {out}"
    );
    let (ok, out, _) = fael(home, repo, &["find", b]);
    assert!(ok, "{out}");
    assert!(out.contains("(closed)"), "B reads closed: {out}");
    let (ok, out, _) = fael(home, repo, &["find", "--kind", "issue", "--json", "--all"]);
    assert!(ok, "{out}");
    assert!(
        out.contains(&format!("\"supersedes\":\"{a}\"")),
        "B's row carries the edge: {out}"
    );
}

#[test]
fn preset_clone_passes_the_five_workflow_smoke_with_no_config_edit() {
    let (home, repo) = scratch();
    let (ok, out, err) = fael(&home, &repo, &["install", "--preset", "multi-agent"]);
    assert!(ok, "{out} {err}");
    let cfg_before = std::fs::read_to_string(repo.join(".fael/config.toml")).unwrap();

    // 1. kickoff on an empty repo: the brief, not an error
    let (ok, _, err) = fael(&home, &repo, &["kickoff"]);
    assert!(ok, "{err}");
    // 2-5. add, duplicate retry, supersede, close — every repeat idempotent
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// x\n").unwrap();
    let (a, b) = add_close_cycle(&home, &repo);
    assert_ledger(&home, &repo, &a, &b);
    // kickoff and doctor stay green with the preset in place
    let (ok, _, err) = fael(&home, &repo, &["kickoff"]);
    assert!(ok, "{err}");
    let (ok, out, err) = fael(&home, &repo, &["doctor"]);
    assert!(ok, "{out} {err}");

    assert_eq!(
        std::fs::read_to_string(repo.join(".fael/config.toml")).unwrap(),
        cfg_before,
        "smoke passes with zero config edits"
    );
}

#[test]
fn preset_two_sessions_both_get_the_row() {
    let (home, repo) = scratch();
    let (ok, out, err) = fael(&home, &repo, &["install", "--preset", "multi-agent"]);
    assert!(ok, "{out} {err}");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// x\n").unwrap();
    let (ok, _, err) = fael(
        &home,
        &repo,
        &[
            "add",
            "issue",
            "xsession row",
            "--title",
            "xs",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let (ok, out, _) = fael(&home, &repo, &["find", "--files", "src/a.rs", "--json"]);
    assert!(ok, "{out}");
    let short: String = out
        .lines()
        .filter(|l| l.starts_with('{'))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["id"].as_str().map(String::from))
        .next()
        .unwrap_or_default();
    assert!(!short.is_empty(), "{out}");
    // pushes name the short id, so match on its prefix
    let short = short[..8].to_string();
    let edit = |s: &str| {
        hook(
            &home,
            &repo,
            &["hook", "edit"],
            &format!(
                "{{\"cwd\":{},\"session\":{s},\"files\":[\"src/a.rs\"]}}",
                serde_json::Value::String(repo.display().to_string())
            ),
        )
    };
    // first session gets the body; a repeat in the same session stays silent
    // (already pushed — the restart-equivalent within one session)
    let (ok, first) = edit("\"s1\"");
    assert!(ok, "{first}");
    assert!(first.contains(&short), "s1 is handed the row: {first}");
    let (ok, again) = edit("\"s1\"");
    assert!(ok, "{again}");
    assert!(!again.contains(&short), "s1 repeat stays silent: {again}");
    // F-XSESSION: a second session still gets the same row
    let (ok, second) = edit("\"s2\"");
    assert!(ok, "{second}");
    assert!(
        second.contains(&short),
        "s2 is handed the row too: {second}"
    );
}

#[test]
fn preset_clone_passes_chunk_and_plan_lint_with_no_config_edit() {
    let (home, repo) = scratch();
    // crew verbs exec the fael-crew binary beside fael — a workspace build
    // puts it there; a partial `-p fael` build does not
    let crew = PathBuf::from(env!("CARGO_BIN_EXE_fael"))
        .with_file_name(format!("fael-crew{}", std::env::consts::EXE_SUFFIX));
    assert!(
        crew.is_file(),
        "{} is missing — run `cargo test --workspace` (or `cargo build --workspace`) first",
        crew.display()
    );
    let (ok, out, err) = fael(&home, &repo, &["install", "--preset", "multi-agent"]);
    assert!(ok, "{out} {err}");
    let cfg_before = std::fs::read_to_string(repo.join(".fael/config.toml")).unwrap();

    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// x\n").unwrap();
    let (ok, out, err) = fael(&home, &repo, &["plan", "new", "smoke", "Smoke plan"]);
    assert!(ok, "{out} {err}");
    let (ok, out, err) = fael(
        &home,
        &repo,
        &[
            "chunk",
            "add",
            "Smoke chunk",
            "--plan",
            "smoke",
            "--scope",
            "src/",
            "--brief",
            "Smoke it",
        ],
    );
    assert!(ok, "{out} {err}");
    let u = out.trim().to_string();
    let (ok, out, err) = fael(&home, &repo, &["plan", "lint", "smoke"]);
    assert!(ok, "{out} {err}");
    assert!(out.contains("no dangling refs"), "{out}");
    // resume: the second start returns the same live run, short
    let (ok, first, err) = fael(&home, &repo, &["chunk", "start", &u, "--session", "S1"]);
    assert!(ok, "{err}");
    let run = first
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .nth(2)
        .unwrap_or_default()
        .to_string();
    assert!(!run.is_empty() && first.contains("Smoke it"), "{first}");
    let (ok, second, err) = fael(&home, &repo, &["chunk", "start", &u, "--session", "S1"]);
    assert!(ok, "{err}");
    assert!(second.contains("(resumed)"), "same live R, short: {second}");
    assert!(
        second.contains(&format!("fael run {run}")),
        "same run id: {second}"
    );
    // done with no change lands in review with the handoff recorded
    let (ok, out, err) = fael(
        &home,
        &repo,
        &[
            "chunk",
            "done",
            &u,
            "smoke done",
            "--left",
            "none",
            "--no-change",
            "--review",
            "clean",
        ],
    );
    assert!(ok, "{out} {err}");
    let (ok, out, _) = fael(&home, &repo, &["chunk", "show", &u]);
    assert!(ok, "{out}");
    assert!(
        out.contains("review") && out.contains("smoke done"),
        "{out}"
    );

    assert_eq!(
        std::fs::read_to_string(repo.join(".fael/config.toml")).unwrap(),
        cfg_before,
        "chunk + lint pass with zero config edits"
    );
}
