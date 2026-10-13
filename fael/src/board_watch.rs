//! `fael board --json --watch` (chunk board-watch, issue 01M4SW254): the app's one long-lived
//! read. Each second it stats what every project's board is built from and rereads only the
//! project whose sources moved; a line goes out only when the board, but for `at`, changed.
//! A source that did not move is served from memory: no git spawn, no jsonl parse, no write.
//! It is the app's child, not a daemon: it exits when the app closes its stdin or stdout.
// ponytail: a fixed 1 s poll of stats, no FSEvents/inotify; a db in a repo off the registry
// is not watched (the registry is reread each tick).

use super::{Project, clock, lists, prs, read, registered};
use crate::plan::db;
use crate::{Args, repo_at};
use fael_core::plan::Store;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

/// Each file's path, size and mtime.
type Stamp = Vec<(PathBuf, u64, Option<SystemTime>)>;
type Parsed = (crate::core::Log, HashSet<String>);
type GitOut = Option<(Option<i32>, String)>;

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static GIT: RefCell<HashMap<(PathBuf, Vec<String>), GitOut>> = RefCell::default();
    static LEDGER: RefCell<HashMap<PathBuf, Rc<Parsed>>> = RefCell::default();
}

/// `git <args>` in `dir`: its exit code and stdout. Under --watch the same call is answered
/// from memory until `dir`'s refs move: every git read the board makes depends on refs only.
pub(crate) fn git_output(dir: &Path, args: &[&str]) -> GitOut {
    let spawn = || {
        let o = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .ok()?;
        Some((
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        ))
    };
    if !ON.get() {
        return spawn();
    }
    let key = (
        dir.to_path_buf(),
        args.iter().map(|a| a.to_string()).collect(),
    );
    if let Some(v) = GIT.with_borrow(|m| m.get(&key).cloned()) {
        return v;
    }
    let v = spawn();
    GIT.with_borrow_mut(|m| m.insert(key, v.clone()));
    v
}

/// A project's parsed ledger and its drifted ids; under --watch, kept until its jsonl or refs move.
pub(super) fn ledger(root: &Path, parse: impl FnOnce() -> Parsed) -> Rc<Parsed> {
    if !ON.get() {
        return Rc::new(parse());
    }
    if let Some(p) = LEDGER.with_borrow(|m| m.get(root).cloned()) {
        return p;
    }
    let p = Rc::new(parse());
    LEDGER.with_borrow_mut(|m| m.insert(root.to_path_buf(), p.clone()));
    p
}

/// What one project's board is built from. `minute` rereads the db once a minute anyway:
/// `stalled` (30 min) and a wait's `until` move with the clock, not with a write.
#[derive(PartialEq, Default)]
struct Keys {
    db: Option<i64>,
    db_file: Stamp,
    files: Stamp,
    git: Stamp,
    minute: String,
}

struct Seen {
    root: PathBuf,
    /// held open only to read `data_version`; never written through
    store: Option<Store>,
    keys: Keys,
    project: Option<Project>,
}

pub(super) fn run(a: &Args) -> Result<ExitCode, String> {
    ON.set(true);
    // the app closes stdin when it quits or dies: fael goes with it, even on a board that
    // never changes (a closed stdout is only seen at the next line)
    std::thread::spawn(|| {
        let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
        std::process::exit(0)
    });
    let (mut seen, mut last) = (Vec::new(), String::new());
    loop {
        if let Some(line) = tick(&mut seen, &mut last, a) {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                return Ok(ExitCode::SUCCESS);
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// One poll: rereads the projects whose keys moved; the compact board line when it differs
/// from `last` but for `at`.
fn tick(seen: &mut Vec<Seen>, last: &mut String, a: &Args) -> Option<String> {
    let (at, stale) = clock();
    let roots = registered();
    if roots.len() != seen.len() || roots.iter().zip(seen.iter()).any(|(r, s)| *r != s.root) {
        let mut old = std::mem::take(seen);
        *seen = roots
            .into_iter()
            .map(|root| match old.iter().position(|s| s.root == root) {
                Some(i) => old.swap_remove(i),
                None => Seen {
                    root,
                    store: None,
                    keys: Keys::default(),
                    project: None,
                },
            })
            .collect();
    }
    let mut moved = last.is_empty();
    for s in seen.iter_mut() {
        let keys = s.keys(&at);
        if keys == s.keys && !last.is_empty() {
            continue;
        }
        if keys.git != s.keys.git {
            GIT.with_borrow_mut(|m| m.retain(|(dir, _), _| !dir.starts_with(&s.root)));
        }
        if keys.git != s.keys.git || keys.files != s.keys.files {
            LEDGER.with_borrow_mut(|m| m.retain(|root, _| !root.starts_with(&s.root)));
        }
        s.project = read(&s.root, &at, &stale, a);
        // ponytail: per project; a run whose worktree another project lists keeps its start branch
        if let Some(p) = s.project.as_mut() {
            prs::live_branches(std::slice::from_mut(p));
        }
        s.keys = keys;
        moved = true;
    }
    if !moved {
        return None;
    }
    let mut b = lists(
        String::new(),
        seen.iter().filter_map(|s| s.project.as_ref()).collect(),
    );
    let body = serde_json::to_string(&b).ok()?;
    if body == *last {
        return None;
    }
    *last = body;
    b.at = at;
    serde_json::to_string(&b).ok()
}

impl Seen {
    fn keys(&mut self, at: &str) -> Keys {
        let minute = at[..16].to_string();
        let Ok(r) = repo_at(&self.root) else {
            return Keys {
                minute,
                ..Keys::default()
            };
        };
        let path = db(&r);
        if self.store.is_none() && path.exists() {
            self.store = Store::open(&path).ok();
        }
        let mut db_file = Stamp::new();
        walk(&path, &|_| true, &mut db_file);
        // the ledger's jsonl (repo and clone journal) and the files open_prs reads beside it
        let mut files = Stamp::new();
        let ledger = |p: &Path| {
            p.extension().is_some_and(|e| e == "jsonl")
                || p.file_name()
                    .is_some_and(|n| n == "shared" || n == "config.toml")
        };
        walk(&r.fael, &ledger, &mut files);
        let common = r.journal.as_deref().and_then(Path::parent);
        if let Some(j) = &r.journal {
            walk(&j.join("log"), &ledger, &mut files);
        }
        // every ref, recursively: a dir's mtime moves only for its own entries, so
        // refs/heads/feat/x or refs/remotes/origin/main never touch refs/
        let mut git = Stamp::new();
        if let Some(c) = common {
            for f in ["HEAD", "packed-refs", "FETCH_HEAD", "refs"] {
                walk(&c.join(f), &|_| true, &mut git);
            }
            let wts = std::fs::read_dir(c.join("worktrees"))
                .into_iter()
                .flatten()
                .flatten();
            for w in wts {
                walk(&w.path().join("HEAD"), &|_| true, &mut git);
            }
        }
        for s in [&mut db_file, &mut files, &mut git] {
            s.sort();
        }
        Keys {
            db: self.store.as_ref().and_then(|s| s.data_version().ok()),
            db_file,
            files,
            git,
            minute,
        }
    }
}

/// Every file at or under `p` that `keep` takes, with its size and mtime.
fn walk(p: &Path, keep: &dyn Fn(&Path) -> bool, out: &mut Stamp) {
    let Ok(m) = std::fs::metadata(p) else {
        return;
    };
    if m.is_dir() {
        for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
            walk(&e.path(), keep, out);
        }
    } else if keep(p) {
        out.push((p.to_path_buf(), m.len(), m.modified().ok()));
    }
}
