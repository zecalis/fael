//! c7 preset (plan fael-secretary): `fael install --preset multi-agent`
//! writes the repo's `.fael/config.toml` from proven primitives only, and a
//! fresh clone with that file passes the 5-workflow smoke (kickoff, add,
//! close, supersede, doctor) plus duplicate/retry idempotence with zero
//! config edits. Cross-session visibility itself is c3's contract
//! (`hook/push_once.rs::two_sessions_both_get_the_row`); the preset pins the
//! budgets that carry it, so this file asserts the preset parses to defaults.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(home: &Path, repo: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("FAEL_STATE_DIR", home.join("state"))
        // no claude CLI on this PATH: client wiring is skipped, the preset
        // lands alone — the fresh-clone shape this chunk is about
        .env(
            "PATH",
            std::env::join_paths([
                Path::new(env!("CARGO_BIN_EXE_fael")).parent().unwrap(),
                Path::new("/usr/bin"),
                Path::new("/bin"),
            ])
            .unwrap(),
        )
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn scratch() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("fael-preset-{}", fael_core::ulid()));
    let (home, repo) = (base.join("home"), base.join("repo"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Test User"],
        &["config", "user.email", "t@example.com"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
    }
    (home, repo)
}

fn id_of(out: &str) -> &str {
    out.split_whitespace().next().unwrap()
}

#[test]
fn preset_body_is_defaults_no_hidden_semantics() {
    let cfg = fael_core::Config::from_toml(fael_core::PRESET_MULTI_AGENT_TOML).unwrap();
    let d = fael_core::Config::default();
    assert_eq!(cfg.kickoff_tokens, d.kickoff_tokens);
    assert_eq!(cfg.find_tokens, d.find_tokens);
    assert_eq!(cfg.push_tokens, d.push_tokens);
    assert_eq!(cfg.push_rows, d.push_rows);
    assert_eq!(cfg.session_decisions, d.session_decisions);
    assert_eq!(cfg.warn_row_tokens, d.warn_row_tokens);
    assert_eq!(cfg.warn_row_chars, d.warn_row_chars);
    assert_eq!(cfg.resolve, d.resolve);
    assert_eq!(cfg.row_bytes, d.row_bytes);
    assert_eq!(cfg.sync_auto, d.sync_auto);
    assert_eq!(cfg.notify_user, d.notify_user);
    assert_eq!(cfg.lang_marker, d.lang_marker);
    assert_eq!(cfg.lang_rows, d.lang_rows);
    assert_eq!(cfg.hint_stop, d.hint_stop);
    assert_eq!(cfg.anchor_prefixes, d.anchor_prefixes);
}

#[test]
fn preset_dry_run_writes_nothing_unknown_preset_rejects() {
    let (home, repo) = scratch();
    let (ok, out, _) = fael(
        &home,
        &repo,
        &["install", "--preset", "multi-agent", "--dry-run"],
    );
    assert!(ok, "{out}");
    assert!(!repo.join(".fael/config.toml").exists(), "{out}");
    let (ok, _, err) = fael(&home, &repo, &["install", "--preset", "nope"]);
    assert!(!ok, "unknown preset must reject");
    assert!(err.contains("unknown preset"), "{err}");
    assert!(!repo.join(".fael/config.toml").exists());
}

#[test]
fn preset_writes_once_leaves_a_hand_tuned_config_alone() {
    let (home, repo) = scratch();
    let (ok, out, _) = fael(&home, &repo, &["install", "--preset", "multi-agent"]);
    assert!(ok, "{out}");
    let path = repo.join(".fael/config.toml");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        fael_core::PRESET_MULTI_AGENT_TOML
    );
    // second run: identical bytes, nothing rewritten
    let (ok, out, _) = fael(&home, &repo, &["install", "--preset", "multi-agent"]);
    assert!(ok, "{out}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        fael_core::PRESET_MULTI_AGENT_TOML
    );
    // a hand-tuned config is never clobbered by the preset
    std::fs::write(&path, "[budget]\npush_rows = 2\n").unwrap();
    let (ok, out, _) = fael(&home, &repo, &["install", "--preset", "multi-agent"]);
    assert!(ok, "{out}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "[budget]\npush_rows = 2\n",
        "{out}"
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
    // 2. add a row
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// x\n").unwrap();
    let (ok, out, err) = fael(
        &home,
        &repo,
        &[
            "add",
            "issue",
            "smoke hot path",
            "--title",
            "smoke",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{err}");
    let a = id_of(&out).to_string();
    // 3. duplicate add (retry lifecycle): same id, zero new rows
    let (ok, out2, _) = fael(
        &home,
        &repo,
        &[
            "add",
            "issue",
            "smoke hot path",
            "--title",
            "smoke",
            "--files",
            "src/a.rs",
        ],
    );
    assert!(ok, "{out2}");
    assert_eq!(id_of(&out2), a.as_str(), "retry names the existing row");
    // 4. supersede A -> B
    let (ok, out, err) = fael(
        &home,
        &repo,
        &[
            "add",
            "issue",
            "smoke hot path v2",
            "--title",
            "smoke v2",
            "--files",
            "src/a.rs",
            "--supersedes",
            &a,
        ],
    );
    assert!(ok, "{err}");
    let b = id_of(&out).to_string();
    // 5. close the head; kickoff and doctor stay green with the preset in place
    let (ok, _, err) = fael(
        &home,
        &repo,
        &[
            "close",
            &b,
            "smoke broke → smoke fixed; guard `src/a_test.rs`",
        ],
    );
    assert!(ok, "{err}");
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
