//! Git plumbing for `fael sync` — the only place sync touches the network
//! or the object store. Core (`fael_core::sync`) never sees these; the flow
//! in `super` never touches a socket. Fetches only store objects, pushes
//! name the commit sha, so the working tree, checked-out branches and
//! `FETCH_HEAD` never move.

use crate::core;
use std::path::Path;
use std::process::{Command, Stdio};

/// One fetched ref: its `meta.json` plus both row streams, parsed the way
/// readers parse (torn tail ignored, bad lines warned, never fatal).
#[derive(Default)]
pub(crate) struct Fetched {
    pub meta: Option<core::sync::Meta>,
    pub rows: Vec<core::Row>,
    pub closes: Vec<core::Row>,
}

/// Fetch `refname` (objects only — never a local ref, the worktree, or the
/// `FETCH_HEAD` a concurrent `git pull` merges) and read its tree:
/// `meta.json` + every `*.jsonl`, closes by filename.
pub(crate) fn fetch_tree(
    root: &Path,
    remote: &str,
    refname: &str,
    sha: &str,
) -> Result<Fetched, String> {
    run(root, &["fetch", "--no-write-fetch-head", remote, refname])?;
    let tree = run(root, &["rev-parse", &format!("{sha}^{{tree}}")])?;
    let mut out = Fetched::default();
    let mut warns = vec![];
    for path in run(root, &["ls-tree", "-r", "--name-only", &tree])?.lines() {
        let body = raw(root, &["cat-file", "-p", &format!("{tree}:{path}")])?;
        if path == "meta.json" {
            out.meta =
                Some(core::sync::Meta::from_json(&body).map_err(|e| format!("{refname}: {e}"))?);
        } else if path.ends_with(".jsonl") {
            let dst = if path.ends_with(".close.jsonl") {
                &mut out.closes
            } else {
                &mut out.rows
            };
            core::parse(body.as_bytes(), path, dst, &mut warns);
        }
    }
    if !warns.is_empty() {
        eprintln!(
            "fael: {} fetched line(s) skipped — first: {}",
            warns.len(),
            warns[0]
        );
    }
    Ok(out)
}

/// `git ls-remote <remote> <ref>` → the tip sha, or `None` when the remote
/// has no such ref yet.
pub(crate) fn ls_remote(
    root: &Path,
    remote: &str,
    refname: &str,
) -> Result<Option<String>, String> {
    let out = run(root, &["ls-remote", remote, refname])?;
    Ok(out
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().next())
        .map(str::to_string))
}

/// `git ls-remote <remote> <prefix>` → `sha\tref` lines, or `None` when the
/// remote (or its fael namespace) is empty. A transport-level failure is an
/// error; an empty namespace is not.
pub(crate) fn ls_prefix(root: &Path, remote: &str, prefix: &str) -> Result<Option<String>, String> {
    match Command::new("git")
        .args(["ls-remote", remote, &format!("{prefix}*")])
        .current_dir(root)
        .output()
        .map_err(|e| format!("fael: git ls-remote: {e}"))?
    {
        o if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            Ok((!s.is_empty()).then_some(s))
        }
        o => Err(format!(
            "fael: git ls-remote: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
    }
}

/// The ref column of a `sha\tref` line.
pub(crate) fn line_ref(line: &str) -> &str {
    line.split_whitespace().nth(1).unwrap_or("")
}

/// The tree the remote tip has (`None` when there is no tip yet) — a sync
/// whose union builds the same tree pushes nothing. The sha is known locally:
/// the union just fetched it.
pub(crate) fn tip_tree(root: &Path, tip: &Option<String>) -> Result<Option<String>, String> {
    let Some(sha) = tip else { return Ok(None) };
    Ok(Some(run(root, &["rev-parse", &format!("{sha}^{{tree}}")])?))
}

/// `files` (paths relative to the ref root, `meta.json` first) as one tree:
/// one blob per file via `hash-object -w`, then one `mktree`. Returns the
/// tree sha — identical journals build identical trees, so a repeat sync
/// compares before it commits.
pub(crate) fn write_tree(root: &Path, files: &[core::sync::TreeFile]) -> Result<String, String> {
    let mut entries = String::new();
    for f in files {
        let blob = stdin(root, &["hash-object", "-w", "--stdin"], &f.body)?;
        entries.push_str(&format!("100644 blob {blob}\t{}\n", f.path));
    }
    stdin(root, &["mktree"], &entries)
}

/// `commit-tree`, parented on the remote tip when there is one. Identity comes
/// from the repo's git config, falling back to `fael` — a sync commit is
/// provenance, never authorship.
pub(crate) fn commit_tree(root: &Path, tree: &str, parent: Option<&str>) -> Result<String, String> {
    let name = crate::git(root, &["config", "user.name"]).unwrap_or_else(|| "fael".into());
    let email =
        crate::git(root, &["config", "user.email"]).unwrap_or_else(|| "fael@localhost".into());
    let mut c = Command::new("git");
    c.args(["commit-tree", tree, "-m", "fael sync"]);
    if let Some(p) = parent {
        c.args(["-p", p]);
    }
    c.env("GIT_AUTHOR_NAME", &name)
        .env("GIT_COMMITTER_NAME", &name)
        .env("GIT_AUTHOR_EMAIL", &email)
        .env("GIT_COMMITTER_EMAIL", &email);
    let o = c
        .current_dir(root)
        .output()
        .map_err(|e| format!("fael: git commit-tree: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "fael: git commit-tree: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Push the commit at `refname`, fast-forward only, never force. `Ok(Some)`
/// on success, `Ok(None)` when the remote moved under us (retry once).
pub(crate) fn push(
    root: &Path,
    remote: &str,
    refname: &str,
    commit: &str,
) -> Result<Option<()>, String> {
    let o = Command::new("git")
        .args(["push", remote, &format!("{commit}:{refname}")])
        .current_dir(root)
        .output()
        .map_err(|e| format!("fael: git push: {e}"))?;
    if o.status.success() {
        return Ok(Some(()));
    }
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    if err.contains("non-fast-forward") || err.contains("fetch first") || err.contains("[rejected]")
    {
        return Ok(None);
    }
    Err(format!("fael: git push: {}", err.trim()))
}

/// Run git, return trimmed stdout; any failure is the stderr, trimmed.
pub(crate) fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    Ok(raw(root, args)?.trim().to_string())
}

/// Run git, return stdout untouched — for blob bodies where the trailing
/// newline is content (`cat-file`), not framing. Callers trim when the
/// output is lines or a single sha.
fn raw(root: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("fael: git {}: {e}", args.join(" ")))?;
    if !o.status.success() {
        return Err(format!(
            "fael: git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Run git with `input` on stdin, return trimmed stdout.
fn stdin(root: &Path, args: &[&str], input: &str) -> Result<String, String> {
    let mut c = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("fael: git {}: {e}", args.join(" ")))?;
    use std::io::Write;
    c.stdin
        .take()
        .ok_or_else(|| "fael: git: no stdin".to_string())?
        .write_all(input.as_bytes())
        .map_err(|e| format!("fael: git {}: {e}", args.join(" ")))?;
    let o = c
        .wait_with_output()
        .map_err(|e| format!("fael: git {}: {e}", args.join(" ")))?;
    if !o.status.success() {
        return Err(format!(
            "fael: git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}
