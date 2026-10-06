//! `fael upgrade` updates the binary by the channel its path says (PLAN-fael-auto-update chunk 2).

#![cfg(unix)]

use std::path::Path;
use std::process::Command;

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// A copy of fael inside a fake brew Cellar, so the channel reads as brew.
fn brew_copy(base: &Path) -> std::path::PathBuf {
    let bin = base.join("Cellar/fael/0.0.1/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_fael"), bin.join("fael")).unwrap();
    bin.join("fael")
}

#[test]
fn upgrade_dry_run_names_the_channel_and_runs_nothing() {
    let base = std::env::temp_dir().join(format!("fael-chan-{}", fael_core::ulid()));
    let home = base.join("home");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    let o = Command::new(brew_copy(&base))
        .args(["upgrade", "--dry-run"])
        .env("HOME", &home)
        .env(
            "PATH",
            format!(
                "{}:/usr/bin:/bin",
                Path::new(env!("CARGO_BIN_EXE_fael"))
                    .parent()
                    .unwrap()
                    .display()
            ),
        )
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        out.contains("channel: brew — runs `brew upgrade zecalis/tap/fael`"),
        "{out}"
    );
    assert!(!home.join(".codex/hooks.json").exists(), "{out}");
}

/// `fael upgrade --yes <args>` from a fake Cellar with a fake `brew` (`script`
/// is its body) first on PATH. Returns (output, base) — `base/ran` is what brew was given.
fn upgrade_via_fake_brew(
    script: &str,
    dirs: &[&str],
    args: &[&str],
) -> (std::process::Output, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let base = std::env::temp_dir().join(format!("fael-chan-{}", fael_core::ulid()));
    let home = base.join("home");
    for d in dirs {
        std::fs::create_dir_all(home.join(d)).unwrap();
    }
    let fake = base.join("fakebin");
    std::fs::create_dir_all(&fake).unwrap();
    let brew = fake.join("brew");
    std::fs::write(
        &brew,
        format!(
            "#!/bin/sh\necho \"$@\" > {}/ran\n{script}\n",
            base.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&brew, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = Command::new(brew_copy(&base))
        .args(["upgrade", "--yes"])
        .args(args)
        .env("HOME", &home)
        .env(
            "PATH",
            format!(
                "{}:{}:/usr/bin:/bin",
                fake.display(),
                Path::new(env!("CARGO_BIN_EXE_fael"))
                    .parent()
                    .unwrap()
                    .display()
            ),
        )
        .output()
        .unwrap();
    (o, base)
}

/// The binary step runs the channel's command, then the wiring (by the PATH fael).
#[test]
fn upgrade_runs_the_channel_command_then_writes_the_wiring() {
    let (o, base) = upgrade_via_fake_brew("", &[".codex"], &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(read(&base.join("ran")).trim(), "upgrade zecalis/tap/fael");
    assert!(base.join("home/.codex/hooks.json").exists());
}

/// A failed channel command stops there: an error, and no wiring is written.
#[test]
fn upgrade_stops_when_the_channel_command_fails() {
    let (o, base) = upgrade_via_fake_brew("exit 1", &[".codex"], &[]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success(), "{err}");
    assert!(
        err.contains("failed") && err.contains("nothing else was changed"),
        "{err}"
    );
    assert!(!base.join("home/.codex/hooks.json").exists());
}

/// `--client` reaches the new binary's wiring pass: only that client is written.
#[test]
fn upgrade_forwards_client_to_the_wiring_pass() {
    let (o, base) = upgrade_via_fake_brew("", &[".codex", ".claude"], &["--client", "codex"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(base.join("home/.codex/hooks.json").exists());
    assert!(!base.join("home/.claude/settings.json").exists());
}
