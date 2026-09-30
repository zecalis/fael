//! `fael sync` against real bare repos (PLAN-fael-journal-transport chunk 4):
//! every done-case of the plan §3 — two clones converge on one journal, two
//! repos never share a ref, two writers push at once, `store = local` warns
//! only at origin, a two-root repo keeps one repo-id, the working tree never
//! moves, and a missing remote / an empty journal behave exactly as
//! [docs/sync-format.md](../../../docs/sync-format.md) says.
//!
//! Thin entry only — the suites sit next to this file:
//! `share` (two clones: same rows, flat ref tree, clean working tree),
//! `namespace` (two repos on one remote; repo-id across clones/branches),
//! `writers` (two writers pushing at the same time),
//! `origin` (the `store = local` + origin warning),
//! `errors` (no remote → error, empty journal → no ref),
//! `private` (public source repo, memory in a separate private remote).

mod errors;
mod namespace;
mod origin;
mod private;
mod share;
mod writers;

use std::path::{Path, PathBuf};
use std::process::Command;

/// `fael <args>` in `dir`, with its own `FAEL_STATE_DIR` **outside** the repo —
/// the working-tree case below asserts a clean `git status`, so session files
/// must never land in it — and with the calling agent's session id cleared.
fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let root = repo_root(dir);
    let name = root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let state = root.parent().unwrap_or(&root).join(format!("{name}-state"));
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("CLAUDE_CODE_SESSION_ID");
    let o = c.output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// The plain remote resolution: `git config fael.remote`, no `--remote` flag.
fn sync(d: &Path) -> (bool, String, String) {
    fael(d, &["sync"])
}

/// The repo root — the first ancestor holding `.git`.
fn repo_root(dir: &Path) -> PathBuf {
    dir.ancestors()
        .find(|p| p.join(".git").exists())
        .unwrap()
        .to_path_buf()
}

fn git(d: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn git_out(d: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A unique scratch directory (never cleaned — the suites match the rest of
/// `fael/tests`, which leave their throwaways in the temp dir).
fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-sync-{name}-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A bare remote — what every test points `fael.remote` at.
fn bare(name: &str) -> PathBuf {
    let d = temp(name);
    git(&d, &["init", "-q", "--bare"]);
    d
}

/// A source repo: one commit and its own git identity (which is the writer id).
fn repo(name: &str, user: &str, email: &str) -> PathBuf {
    let d = temp(name);
    git(&d, &["init", "-q"]);
    git(&d, &["config", "user.name", user]);
    git(&d, &["config", "user.email", email]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    d
}

/// A clone of `src` with its own identity — a clone carries no local config,
/// so every test names the writer it wants (same identity = same writer id).
fn clone(src: &Path, name: &str, user: &str, email: &str) -> PathBuf {
    let d = temp(name);
    let from = src.to_str().unwrap().to_string();
    let to = d.to_str().unwrap().to_string();
    let cwd = std::env::temp_dir();
    git(&cwd, &["clone", "-q", &from, &to]);
    git(&d, &["config", "user.name", user]);
    git(&d, &["config", "user.email", email]);
    d
}

/// `git config fael.remote <url>` — per machine, in `.git/config`, never committed.
fn point(d: &Path, url: &Path) {
    git(d, &["config", "fael.remote", url.to_str().unwrap()]);
}

/// `fael add note <text> --files doc:sync` → the id it filed.
fn add(d: &Path, text: &str) -> String {
    let (ok, out, err) = fael(d, &["add", "note", text, "--files", "doc:sync"]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// Every row `find --all --json` lists, parsed — close rows too.
fn rows(d: &Path) -> Vec<serde_json::Value> {
    let (ok, out, err) = fael(d, &["find", "--all", "--json"]);
    assert!(ok, "{err}");
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect()
}

/// The ids the repo lists, unsorted.
fn ids(d: &Path) -> Vec<String> {
    rows(d)
        .into_iter()
        .filter_map(|v| v["id"].as_str().map(String::from))
        .collect()
}

/// The ids, sorted and deduped — `len()` shrinking means a ULID listed twice.
fn unique_ids(d: &Path) -> Vec<String> {
    let mut seen = ids(d);
    seen.sort();
    seen.dedup();
    seen
}

/// The cached repo-id (written by the first sync), or `None` before one.
fn repoid(d: &Path) -> Option<String> {
    let o = Command::new("git")
        .args(["config", "fael.repoid"])
        .current_dir(d)
        .output()
        .unwrap();
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Every `refs/fael/` ref the remote holds, sorted — the whole fael namespace.
fn fael_refs(remote: &Path) -> Vec<String> {
    let url = remote.to_str().unwrap().to_string();
    let mut refs: Vec<String> = git_out(remote, &["ls-remote", &url])
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
        .filter(|r| r.starts_with("refs/fael/"))
        .collect();
    refs.sort();
    refs
}

/// The tip sha of one ref, matched by its exact name (patterns prefix-match).
fn tip(remote: &Path, rname: &str) -> String {
    let url = remote.to_str().unwrap().to_string();
    git_out(remote, &["ls-remote", &url, rname])
        .lines()
        .find_map(|l| {
            let mut p = l.split_whitespace();
            let sha = p.next()?;
            (p.next() == Some(rname)).then(|| sha.to_string())
        })
        .unwrap_or_else(|| panic!("no tip on the remote for {rname}"))
}

/// The ref's committed files — the flat layout, as pushed.
fn ref_files(remote: &Path, rname: &str) -> Vec<String> {
    let sha = tip(remote, rname);
    git_out(remote, &["ls-tree", "-r", "--name-only", &sha])
        .lines()
        .map(str::to_string)
        .collect()
}

/// The ref's committed `meta.json`, parsed.
fn ref_meta(remote: &Path, rname: &str) -> serde_json::Value {
    let sha = tip(remote, rname);
    let body = git_out(remote, &["cat-file", "-p", &format!("{sha}:meta.json")]);
    serde_json::from_str(&body).unwrap()
}

/// Every row body that ref carries — one writer's journal, as committed.
fn ref_body(remote: &Path, rname: &str) -> String {
    let sha = tip(remote, rname);
    let mut body = String::new();
    for f in ref_files(remote, rname) {
        if f.ends_with(".jsonl") {
            body.push_str(&git_out(remote, &["cat-file", "-p", &format!("{sha}:{f}")]));
            body.push('\n');
        }
    }
    body
}

/// The writer id a test repo files rows under — the same derivation `fael`
/// uses (slug of the name + hash of the email; the host never enters it).
fn writer(user: &str, email: &str) -> String {
    fael_core::writer_id(user, Some(email), "")
}
