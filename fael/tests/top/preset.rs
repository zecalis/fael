//! c7 preset (plan fael-secretary): `fael install --preset multi-agent`
//! writes the repo's `.fael/config.toml` from proven primitives only.
//! The acceptance matrix lives in `preset_matrix.rs` on these helpers.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn fael(home: &Path, repo: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("FAEL_STATE_DIR", home.join("state"))
        // hook-session ids never inherit from the test runner's own shell
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("OPENCODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("FAEL_BURST_MS")
        .env("FAEL_NO_AUTO_UPDATE", "1")
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

/// A hook call with piped stdin — the edit push behind F-XSESSION.
pub(crate) fn hook(home: &Path, repo: &Path, args: &[&str], stdin: &str) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(repo)
        .env("HOME", home)
        .env("FAEL_STATE_DIR", home.join("state"))
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("OPENCODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("FAEL_BURST_MS")
        .env("FAEL_NO_AUTO_UPDATE", "1")
        .env(
            "PATH",
            std::env::join_paths([
                Path::new(env!("CARGO_BIN_EXE_fael")).parent().unwrap(),
                Path::new("/usr/bin"),
                Path::new("/bin"),
            ])
            .unwrap(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

pub(crate) fn scratch() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("fael-preset-{}", fael_core::ulid()));
    let (home, repo) = (base.join("home"), base.join("repo"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repo).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "Test User"],
        &["config", "user.email", "t@example.com"],
        &["commit", "-q", "--allow-empty", "-m", "init"],
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

pub(crate) fn id_of(out: &str) -> &str {
    out.split_whitespace().next().unwrap()
}

#[test]
fn preset_body_is_defaults_no_hidden_semantics() {
    let cfg = fael_core::Config::from_toml(fael_core::PRESET_MULTI_AGENT_TOML).unwrap();
    assert_eq!(cfg, fael_core::Config::default());
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
