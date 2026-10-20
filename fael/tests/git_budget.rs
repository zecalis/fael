//! Git-spawn budget (decision 01M5CRPS, fael:01M5CPNJ): a hot path spawns a fixed
//! number of `git` processes however many branches and notes the repo holds, and
//! the hook push path spawns none. Counted through a PATH shim, so a spawn from
//! anywhere in the binary shows. The bound is on the warm run — the second pull,
//! once the caches of facts that are a pure function of object ids are written;
//! a cold pull proves each branch it has not seen, once.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `kickoff` = for-each-ref ×2, rev-list --first-parent, and the squash proof's
/// cat-file, merge-base --octopus, rev-list --parents.
const KICKOFF_MAX: usize = 6;
/// `add` = the stamp (config, symbolic-ref, rev-parse) plus the same pull.
const ADD_MAX: usize = 9;
/// `find` pulls the same way kickoff does.
const FIND_MAX: usize = 6;
/// `hook session-start` = the same pull plus the user-name probe.
const SESSION_MAX: usize = 7;
/// `close` = the stamp alone: trunk probe, user name, branch, short sha.
const CLOSE_MAX: usize = 5;

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(d)
        .stdout(Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn shim(d: &Path) -> (PathBuf, String) {
    let dir = d.join(".shim");
    std::fs::create_dir_all(&dir).unwrap();
    let real = Command::new("which").arg("git").output().unwrap().stdout;
    let real = String::from_utf8(real).unwrap();
    let log = dir.join("spawned");
    let script = dir.join("git");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
            log.display(),
            real.trim()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
    (log, path)
}

/// Run fael with every `git` it starts counted; the spawned command lines.
fn spawns(d: &Path, args: &[&str], stdin: &str) -> Vec<String> {
    use std::io::Write;
    let (log, path) = shim(d);
    let _ = std::fs::remove_file(&log);
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .env("PATH", path)
        .env("FAEL_STATE_DIR", d.join(".state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .args(args)
        .current_dir(d)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    assert!(c.wait().unwrap().success(), "fael {args:?}");
    let seen = std::fs::read_to_string(&log).unwrap_or_default();
    seen.lines().map(str::to_string).collect()
}

/// `n` branches cut from different points of main, each with a note, a third
/// merged, a third squashed, a third open; every fourth then deleted.
fn repo(n: usize) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-budget-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("base"), "base\n").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-qm", "init"]);
    for i in 0..n {
        let b = format!("feat/b{i}");
        let f = format!("f{i}");
        git(&d, &["switch", "-qc", &b, "main"]);
        std::fs::write(d.join(&f), "x\n").unwrap();
        git(&d, &["add", &f]);
        git(&d, &["commit", "-qm", &format!("work {i}")]);
        spawns(
            &d,
            &["add", "note", &format!("stopped at {i}"), "--files", &f],
            "",
        );
        git(&d, &["switch", "-q", "main"]);
        match i % 3 {
            0 => git(
                &d,
                &["merge", "-q", "--no-ff", "-m", &format!("merge {i}"), &b],
            ),
            1 => {
                git(&d, &["merge", "-q", "--squash", &b]);
                git(&d, &["commit", "-qm", &format!("squash {i}")]);
            }
            _ => {}
        }
        if i % 4 == 3 {
            git(&d, &["branch", "-qD", &b]);
        }
    }
    git(&d, &["switch", "-qc", "work", "main"]);
    d
}

fn warm(d: &Path, args: &[&str]) -> Vec<String> {
    spawns(d, args, "");
    spawns(d, args, "")
}

#[test]
fn kickoff_spawns_a_fixed_number_however_many_branches() {
    for n in [4, 12] {
        let d = repo(n);
        let seen = warm(&d, &["kickoff"]);
        assert!(
            seen.len() <= KICKOFF_MAX,
            "{n} branches:\n{}",
            seen.join("\n")
        );
    }
}

#[test]
fn add_spawns_a_fixed_number_however_many_branches_and_notes() {
    for n in [4, 12] {
        let d = repo(n);
        let args = ["add", "note", "one more", "--files", "f0"];
        let seen = warm(&d, &args);
        assert!(seen.len() <= ADD_MAX, "{n} branches:\n{}", seen.join("\n"));
    }
}

#[test]
fn find_spawns_a_fixed_number_however_many_branches() {
    for n in [4, 12] {
        let d = repo(n);
        for args in [
            &["find", "stopped"][..],
            &["find", "--all"],
            &["find", "--files", "f0"],
        ] {
            let seen = warm(&d, args);
            assert!(
                seen.len() <= FIND_MAX,
                "fael {args:?}, {n} branches:\n{}",
                seen.join("\n")
            );
        }
    }
}

#[test]
fn session_start_spawns_a_fixed_number_however_many_branches() {
    for n in [4, 12] {
        let d = repo(n);
        let input = format!(r#"{{"cwd":"{}"}}"#, d.display());
        spawns(&d, &["hook", "session-start"], &input);
        let seen = spawns(&d, &["hook", "session-start"], &input);
        assert!(
            seen.len() <= SESSION_MAX,
            "{n} branches:\n{}",
            seen.join("\n")
        );
    }
}

/// The push path owns no spawn, and `close` only the stamp of its line.
#[test]
fn the_push_path_spawns_no_git_and_close_only_its_stamp() {
    let d = repo(8);
    let cwd = d.display();
    let edit = format!(r#"{{"cwd":"{cwd}","files":["f1"]}}"#);
    let search = format!(
        r#"{{"cwd":"{cwd}","session_id":"s","tool_name":"Grep","tool_input":{{"pattern":"x"}},"tool_response":"f1"}}"#
    );
    let stop = format!(r#"{{"cwd":"{cwd}"}}"#);
    for (args, input) in [
        (&["hook", "edit"][..], &edit),
        (&["hook", "read"], &edit),
        (&["hook", "search", "--client", "claude"], &search),
        (&["hook", "stop"], &stop),
    ] {
        let seen = spawns(&d, args, input);
        assert!(seen.is_empty(), "fael {args:?}:\n{}", seen.join("\n"));
    }
    let found = std::process::Command::new(env!("CARGO_BIN_EXE_fael"))
        .env("FAEL_STATE_DIR", d.join(".state"))
        .args(["find", "stopped at 1"])
        .current_dir(&d)
        .output()
        .unwrap();
    let found = String::from_utf8_lossy(&found.stdout);
    let id = found
        .split_once("- [")
        .and_then(|(_, r)| r.split_once(']'))
        .map_or("", |(id, _)| id);
    assert!(!id.is_empty(), "a row to close:\n{found}");
    let seen = spawns(&d, &["close", id, "done"], "");
    assert!(seen.len() <= CLOSE_MAX, "close:\n{}", seen.join("\n"));
}
