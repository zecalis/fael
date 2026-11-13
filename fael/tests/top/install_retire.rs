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
    let out = install(&home, &["--client", "claude"]);
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
    let out = install(&home, &["--client", "claude"]);
    assert!(!out.contains("retired") && !out.contains("wrote"), "{out}");
}
