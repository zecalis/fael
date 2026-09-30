//! Auto sync (PLAN-fael-journal-transport chunk 6): session start and the Stop hook
//! start one detached `fael sync` per session and newest row when `fael.remote` is set, the
//! `[sync] auto = false` switch silences it, and a dead remote never holds the
//! turn. The sync is a background child, so tests poll the bare remote.

use super::*;
use std::time::{Duration, Instant};

fn bare() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-hook-bare-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "--bare"]);
    d
}

fn point(d: &Path, url: &Path) {
    git(d, &["config", "fael.remote", url.to_str().unwrap()]);
}

fn add(d: &Path, text: &str) {
    let (ok, _, err) = fael(d, &["add", "note", text, "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
}

/// A stop event that lets the turn through (the session is no transcript and
/// no timestamp, so nothing can block) — returns the hook's reply.
fn stop(d: &Path, session: &str) -> String {
    let ev = format!(r#"{{"cwd":{},"session":"{session}"}}"#, json(d));
    let (ok, out, _) = fael(d, &["hook", "stop"], &ev);
    assert!(ok);
    out
}

fn tips(remote: &Path) -> String {
    git(remote, &["for-each-ref", "refs/fael/"])
}

/// Wait for the background sync to land a ref.
fn wait_ref(remote: &Path) -> String {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        let t = tips(remote);
        if !t.is_empty() {
            return t;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no auto sync within 15s");
}

/// Long enough for a spawned sync to have finished if it were going to run.
fn settle() {
    std::thread::sleep(Duration::from_millis(1500));
}

#[test]
fn a_stop_syncs_again_when_a_newer_row_was_filed() {
    let d = repo();
    let remote = bare();
    point(&d, &remote);
    add(&d, "filed before the stop");
    assert!(stop(&d, "s1").contains(r#""block":false"#));
    let first = wait_ref(&remote);

    // same session, nothing new: the second stop starts no sync
    stop(&d, "s1");
    settle();
    assert_eq!(tips(&remote), first);

    // a row filed after the first stop ships at the next stop of the same session
    add(&d, "filed after the first stop");
    stop(&d, "s1");
    let end = Instant::now() + Duration::from_secs(15);
    while tips(&remote) == first && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_ne!(tips(&remote), first, "the newer row was not shipped");
}

#[test]
fn the_off_switch_keeps_the_hook_silent() {
    let d = repo();
    let remote = bare();
    point(&d, &remote);
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "[sync]\nauto = false\n").unwrap();
    add(&d, "a row the switch keeps local");
    stop(&d, "s1");
    settle();
    assert_eq!(tips(&remote), "", "auto = false: nothing pushed");
}

/// ssh prompts read `/dev/tty`, past `GIT_TERMINAL_PROMPT` — the child's ssh
/// must run in BatchMode. A fake `ssh` on PATH records what git ran.
#[cfg(unix)]
#[test]
fn an_ssh_remote_runs_in_batch_mode() {
    use std::os::unix::fs::PermissionsExt;
    let d = repo();
    let bin = std::env::temp_dir().join(format!("fael-hook-ssh-{}", fael_core::ulid()));
    std::fs::create_dir_all(&bin).unwrap();
    let ssh = bin.join("ssh");
    let script = "#!/bin/sh\nd=$(dirname \"$0\")\necho \"$@\" > \"$d/args.tmp\" && mv \"$d/args.tmp\" \"$d/args\"\nexit 255\n";
    std::fs::write(&ssh, script).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(
        &d,
        &["config", "fael.remote", "ssh://fael.invalid/memory.git"],
    );
    add(&d, "a row for the ssh remote");

    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let ev = format!(r#"{{"cwd":{},"session":"s1"}}"#, json(&d));
    let envs = [("PATH", path.as_str()), ("GIT_SSH_COMMAND", "ssh")];
    assert!(fael_env(&d, &["hook", "stop"], &ev, &envs).0);
    let args = bin.join("args");
    let end = Instant::now() + Duration::from_secs(15);
    while !args.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    let got = std::fs::read_to_string(&args).expect("git never ran ssh");
    assert!(got.contains("BatchMode=yes"), "{got}");
}

#[test]
fn no_remote_or_a_dead_remote_still_ends_the_turn() {
    let d = repo();
    add(&d, "no remote configured");
    assert!(stop(&d, "s1").contains(r#""block":false"#));
    settle();
    assert!(!logs(&d).any(|_| true), "nothing was spawned");

    git(&d, &["config", "fael.remote", "/nonexistent/fael-remote"]);
    let t = Instant::now();
    assert!(stop(&d, "s2").contains(r#""block":false"#));
    assert!(t.elapsed() < Duration::from_secs(5), "the hook never waits");
}

#[test]
fn session_start_ingests_a_teammates_row_and_the_stop_after_it_stays_quiet() {
    let d = repo();
    let remote = bare();
    point(&d, &remote);
    // a teammate: a clone with its own identity, synced by hand
    let t = std::env::temp_dir().join(format!("fael-hook-mate-{}", fael_core::ulid()));
    git(
        &d,
        &["clone", "-q", d.to_str().unwrap(), t.to_str().unwrap()],
    );
    git(&t, &["config", "user.email", "mate@example.com"]);
    point(&t, &remote);
    add(&t, "the teammate decided this");
    let (ok, _, err) = fael(&t, &["sync"], "");
    assert!(ok, "{err}");
    let mate = tips(&remote);
    add(&d, "left behind after the last session's final stop");

    let ev = format!(r#"{{"cwd":{},"session":"s1"}}"#, json(&d));
    assert!(fael(&d, &["hook", "session-start"], &ev).0);
    let end = Instant::now() + Duration::from_secs(15);
    let found = || {
        fael(&d, &["find", "teammate"], "")
            .1
            .contains("teammate decided")
    };
    while !found() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(found(), "session start never ingested the teammate's row");

    // it also shipped the row the last session left behind
    while tips(&remote).lines().count() < 2 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    let after_start = tips(&remote);
    assert!(after_start.contains(mate.trim()) && after_start.lines().count() == 2);
    // the session-start sync already ran for this newest row: the stop starts none
    stop(&d, "s1");
    settle();
    assert_eq!(tips(&remote), after_start);
}

/// The `auto-sync-<hash>.log` files in this repo's state dir.
fn logs(d: &Path) -> impl Iterator<Item = std::path::PathBuf> {
    std::fs::read_dir(d.join("state"))
        .into_iter()
        .flatten()
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("auto-sync-"))
        })
}
