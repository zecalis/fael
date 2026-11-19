//! `fael install` retires the Claude matchers that no longer fire (Read,
//! Glob|Grep|Bash): split out of `install.rs` (file-size ratchet).

use std::path::Path;
use std::process::Command;

fn install(home: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("install")
        .args(args)
        .env("HOME", home)
        .env("FAEL_STATE_DIR", home.join("state"))
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
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// Wiring an older install left — a `Read` fael hook, `Glob|Grep|Bash` on search —
/// is rewritten to the shell-only table; a hook of someone else's in those
/// groups stays, and a second run changes nothing.
#[test]
fn install_retires_the_read_and_glob_grep_matchers() {
    let home = std::env::temp_dir().join(format!("fael-install-{}", fael_core::ulid()));
    let claude = home.join(".claude/settings.json");
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(
        &claude,
        r#"{"hooks": {"PostToolUse": [
          {"matcher": "Read", "hooks": [{"type": "command", "command": "fael hook read --client claude", "timeout": 10}, {"type": "command", "command": "bun /x/other.ts hook-read"}]},
          {"matcher": "Glob|Grep|Bash", "hooks": [{"type": "command", "command": "fael hook search --client claude", "timeout": 10}]}]}}"#,
    )
    .unwrap();
    let out = install(&home, &[]);
    assert!(out.contains("PostToolUse(Read) retired"), "{out}");
    let v: serde_json::Value = serde_json::from_str(&read(&claude)).unwrap();
    let groups = v["hooks"]["PostToolUse"].as_array().unwrap();
    let matchers: Vec<_> = groups
        .iter()
        .filter_map(|g| g["matcher"].as_str())
        .collect();
    assert!(!matchers.contains(&"Glob|Grep|Bash"), "{matchers:?}");
    let s = read(&claude);
    assert!(!s.contains("hook read"), "{s}");
    assert!(s.contains("bun /x/other.ts hook-read"), "{s}");
    assert_eq!(s.matches("hook search").count(), 1, "{s}");
    let search = groups
        .iter()
        .find(|g| {
            g["hooks"][0]["command"]
                .as_str()
                .is_some_and(|c| c.contains("hook search"))
        })
        .unwrap();
    assert_eq!(search["matcher"], "Bash");
    let out = install(&home, &[]);
    assert!(!out.contains("retired") && !out.contains("wrote"), "{out}");
}

fn run(home: &Path, cmd: &str, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg(cmd)
        .args(args)
        .env("HOME", home)
        .env("FAEL_STATE_DIR", home.join("state"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// Native Windows has no HOME (only USERPROFILE) — install must still find
/// the home dir. `--dry-run` is retired: it rejects, and the home lookup
/// behind it still holds (a machine with no client installed would fail
/// later with "found no Claude Code", so only the reject is asserted here).
#[test]
fn install_retired_dry_run_rejects() {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        // never the developer's real usage log or session (fael:01M4F3G0)
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join(format!("fael-test-state-{}", std::process::id())),
        )
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("FAEL_SESSION")
        .args(["install", "--dry-run"])
        .env_remove("HOME")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "retired --dry-run was accepted");
    assert!(err.contains("takes no --dry-run"), "{err}");
}

/// Retired scoping flags reject instead of silently narrowing: install
/// wires every client it finds, no `--client`; `--yes` is gone entirely.
#[test]
fn install_retired_client_and_yes_reject() {
    let home = std::env::temp_dir().join(format!("fael-install-{}", fael_core::ulid()));
    for (args, want) in [
        (&["--client", "claude"][..], "takes no --client"),
        (&["--yes"][..], "unknown flag --yes"),
    ] {
        let o = Command::new(env!("CARGO_BIN_EXE_fael"))
            .arg("install")
            .args(args)
            .env("HOME", &home)
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success(), "{args:?} was accepted");
        assert!(err.contains(want), "{args:?}: {err}");
    }
}

#[test]
fn upgrade_applies_then_settles() {
    let home = std::env::temp_dir().join(format!("fael-upgrade-{}", fael_core::ulid()));
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    // retired: `--dry-run` rejects instead of previewing
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("upgrade")
        .arg("--dry-run")
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(!o.status.success(), "retired --dry-run was accepted");
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("takes no --dry-run"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!home.join(".codex/hooks.json").exists());
    // no terminal here, so `update` cannot ask and applies
    let done = run(&home, "update", &[]);
    assert!(
        done.contains("applied") && done.contains("trust the new hooks"),
        "{done}"
    );
    assert!(home.join(".codex/hooks.json").exists(), "{done}");
    let again = run(&home, "upgrade", &[]);
    assert!(again.contains("up to date — nothing to change"), "{again}");
    // "hooks.json" in a path also holds "/hooks" — match the note itself
    assert!(!again.contains("trust the new hooks"), "{again}");
}
