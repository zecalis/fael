//! `fael upgrade --auto`, the detached self-update child (PLAN-fael-auto-update
//! chunk 3): a release tag is installed only after it was seen for a day, by the
//! channel's command, and the result lands in `update.json`. A local repo with
//! tags stands in for GitHub (`FAEL_UPDATE_REMOTE`), a fake `brew` and a fake
//! `fael` for the channel and the new binary.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Run {
    base: PathBuf,
    ok: bool,
}

impl Run {
    fn json(&self) -> String {
        std::fs::read_to_string(self.base.join("state/update.json")).unwrap_or_default()
    }
    fn brew_ran(&self) -> bool {
        self.base.join("ran").exists()
    }
}

fn script(p: &Path, body: &str) {
    std::fs::write(p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A fake Cellar copy of fael runs `upgrade --auto`. `tags` are the remote's
/// release tags, `state` the seeded update.json, `brew` the fake brew's body,
/// `new_version` what the "new binary" reports.
fn auto(tags: &[&str], state: &str, brew: &str, new_version: &str, envs: &[(&str, &str)]) -> Run {
    let base = std::env::temp_dir().join(format!("fael-auto-{}", fael_core::ulid()));
    let remote = base.join("remote");
    std::fs::create_dir_all(&remote).unwrap();
    let git = |args: &[&str]| {
        let s = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&remote)
            .status()
            .unwrap();
        assert!(s.success());
    };
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "x"]);
    for t in tags {
        git(&["tag", t]);
    }
    let fake = base.join("fakebin");
    std::fs::create_dir_all(&fake).unwrap();
    script(
        &fake.join("brew"),
        &format!("echo \"$@\" > {}/ran\n{brew}", base.display()),
    );
    script(
        &fake.join("fael"),
        &format!("[ \"$1\" = --version ] && echo 'fael {new_version}'\nexit 0"),
    );
    std::fs::create_dir_all(base.join("state")).unwrap();
    if !state.is_empty() {
        std::fs::write(base.join("state/update.json"), state).unwrap();
    }
    let bin = base.join("Cellar/fael/0.0.1/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_fael"), bin.join("fael")).unwrap();
    let mut c = Command::new(bin.join("fael"));
    c.args(["upgrade", "--auto"])
        .env("HOME", base.join("home"))
        .env("FAEL_STATE_DIR", base.join("state"))
        .env("FAEL_UPDATE_REMOTE", &remote)
        .env_remove("FAEL_NO_AUTO_UPDATE")
        .env("PATH", format!("{}:/usr/bin:/bin", fake.display()));
    for (k, v) in envs {
        c.env(k, v);
    }
    let ok = c.output().unwrap().status.success();
    Run { base, ok }
}

/// Seen a day ago, checked again now: the release is old enough to install.
const SEEN: &str = r#"{"checked_at":200000,"seen_at":100000,"to":"99.0.0"}"#;

#[test]
fn a_new_tag_is_recorded_and_waits_a_day() {
    let r = auto(&["v0.0.1", "v99.0.0"], "", "", "99.0.0", &[]);
    assert!(r.ok);
    assert!(r.json().contains("\"to\":\"99.0.0\""), "{}", r.json());
    assert!(!r.json().contains("\"updated\""), "{}", r.json());
    assert!(!r.brew_ran());
}

#[test]
fn a_tag_seen_for_a_day_is_installed_by_the_channel() {
    let r = auto(&["v99.0.0"], SEEN, "", "99.0.0", &[]);
    assert!(r.ok);
    assert_eq!(
        std::fs::read_to_string(r.base.join("ran")).unwrap().trim(),
        "upgrade zecalis/tap/fael"
    );
    assert!(r.json().contains("\"result\":\"updated\""), "{}", r.json());
}

#[test]
fn a_failed_command_and_a_lagging_channel_are_reported_not_hidden() {
    let r = auto(&["v99.0.0"], SEEN, "exit 1", "99.0.0", &[]);
    assert!(
        r.json().contains("\"result\":\"failed\"") && r.json().contains("failed"),
        "{}",
        r.json()
    );
    // brew exits 0 but the formula is not out yet: the binary is still the old one
    let r = auto(&["v99.0.0"], SEEN, "", "0.0.1", &[]);
    assert!(
        r.json().contains("\"result\":\"failed\"") && r.json().contains("still 0.0.1"),
        "{}",
        r.json()
    );
}

#[test]
fn off_offline_and_nothing_newer_run_nothing() {
    let off = auto(
        &["v99.0.0"],
        SEEN,
        "",
        "99.0.0",
        &[("FAEL_NO_AUTO_UPDATE", "1")],
    );
    assert!(off.ok && !off.brew_ran());

    let cfg = std::env::temp_dir().join(format!("fael-auto-cfg-{}", fael_core::ulid()));
    std::fs::create_dir_all(cfg.join("fael")).unwrap();
    std::fs::write(cfg.join("fael/config.toml"), "auto_update = false").unwrap();
    let off = auto(
        &["v99.0.0"],
        SEEN,
        "",
        "99.0.0",
        &[("XDG_CONFIG_HOME", cfg.to_str().unwrap())],
    );
    assert!(off.ok && !off.brew_ran());

    let gone = auto(
        &["v99.0.0"],
        SEEN,
        "",
        "99.0.0",
        &[("FAEL_UPDATE_REMOTE", "/nonexistent/remote")],
    );
    assert!(
        gone.ok && !gone.brew_ran() && !gone.json().contains("result\":\"f"),
        "{}",
        gone.json()
    );

    let none = auto(&["v0.0.1"], SEEN, "", "99.0.0", &[]);
    assert!(none.ok && !none.brew_ran());
}
