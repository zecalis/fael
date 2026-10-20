//! `fael board --json --watch` (chunk board-watch): idle spawns nothing and writes nothing, a
//! db write rereads that project without git, a moved ref or worktree branch prints a line,
//! and a closed stdin ends it. Unix: git is counted through a wrapper script on PATH.
#![cfg(unix)]

mod common;

use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

fn run(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn repo(tmp: &Path, name: &str, state: &Path) -> PathBuf {
    let d = tmp.join(name);
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    run(
        &d,
        state,
        &["chunk", "add", &format!("{name} one"), "--brief", "b"],
    );
    d
}

/// The next board line within `secs`, parsed; None when none came.
fn line(rx: &Receiver<String>, secs: u64) -> Option<Value> {
    let l = rx.recv_timeout(Duration::from_secs(secs)).ok()?;
    assert!(!l.contains('\n'), "one compact line");
    Some(serde_json::from_str(&l).unwrap())
}

fn project<'a>(v: &'a Value, root: &Path) -> &'a Value {
    let all = v["projects"].as_array().unwrap();
    all.iter()
        .find(|p| p["root"] == root.to_string_lossy().as_ref())
        .unwrap()
}

/// `fael board --json --watch` in `tmp`, with every git it spawns counted into `count`; its
/// stdout lines come through the receiver.
fn watch(tmp: &Path, state: &Path, count: &Path) -> (Child, Receiver<String>) {
    let real = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let wrapper = format!(
        "#!/bin/sh\necho x >> '{}'\nexec '{}' \"$@\"\n",
        count.display(),
        real.trim()
    );
    std::fs::write(bin.join("git"), wrapper).unwrap();
    Command::new("chmod")
        .args(["+x"])
        .arg(bin.join("git"))
        .status()
        .unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut child = Command::new(common::fael_bin())
        .args(["board", "--json", "--watch"])
        .current_dir(tmp)
        .env("FAEL_STATE_DIR", state)
        .env("PATH", path)
        .env_remove("FAEL_DIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let (tx, rx) = channel();
    let out = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(l);
        }
    });
    (child, rx)
}

#[test]
fn watch_rereads_only_what_moved() {
    let tmp = std::env::temp_dir().join(format!("fael-board-watch-{}", fael_core::ulid()));
    let state = tmp.join("state");
    let (a, b) = (repo(&tmp, "a", &state), repo(&tmp, "b", &state));
    // refs/remotes/origin/ exists before the watch: a later ref there leaves refs/ untouched
    git(&a, &["update-ref", "refs/remotes/origin/old", "main"]);
    let count = tmp.join("git-calls");
    let calls = || {
        std::fs::read_to_string(&count)
            .unwrap_or_default()
            .lines()
            .count()
    };
    let (mut child, rx) = watch(&tmp, &state, &count);

    let v = line(&rx, 10).expect("a line at start");
    assert_eq!(v["v"], 1);
    assert_eq!(v["projects"].as_array().unwrap().len(), 2);
    assert!(project(&v, &a)["base"].is_null());
    let db = a.join(".git/fael/crew.db");
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let (spawned, wal) = (calls(), size(&db.with_extension("db-wal")));

    // idle: no line, no git, nothing written
    assert!(line(&rx, 3).is_none(), "a line on an idle board");
    assert_eq!(calls(), spawned, "git spawned while idle");
    assert_eq!(
        size(&db.with_extension("db-wal")),
        wal,
        "the watch wrote crew.db"
    );

    // a db write in a: a line with it, and no git (refs did not move)
    run(&a, &state, &["chunk", "add", "a two", "--brief", "b"]);
    let v = line(&rx, 5).expect("a line after a write");
    let titles: Vec<&str> = project(&v, &a)["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["title"].as_str())
        .collect();
    assert!(titles.contains(&"a two"), "{titles:?}");
    assert_eq!(calls(), spawned, "a db write respawned git");
    assert_eq!(project(&v, &b)["chunks"].as_array().unwrap().len(), 1);

    // a nested ref (refs/remotes/origin/HEAD) moves only refs/remotes/origin/
    git(&a, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(
        &a,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    let v = line(&rx, 5).expect("a line after a nested ref moved");
    assert_eq!(project(&v, &a)["base"], "origin/main");

    // a worktree switching to a nested branch moves only worktrees/<wt>/HEAD
    let wt = tmp.join("a-wt");
    git(
        &a,
        &["worktree", "add", "-q", "-b", "feat/deep/x"]
            .iter()
            .copied()
            .chain([wt.to_str().unwrap()])
            .collect::<Vec<_>>(),
    );
    line(&rx, 5).expect("a line after a worktree was added");
    git(&wt, &["switch", "-q", "-c", "feat/deep/y"]);
    let v = line(&rx, 5).expect("a line after a worktree switched branch");
    let branches: Vec<&str> = project(&v, &a)["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["branch"].as_str())
        .collect();
    assert!(branches.contains(&"feat/deep/y"), "{branches:?}");

    // the app quits: stdin closes, the watch ends
    drop(child.stdin.take());
    let gone = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        child.try_wait().unwrap().is_some()
    });
    assert!(gone, "the watch outlived its stdin");
    let _ = std::fs::remove_dir_all(&tmp);
}
