//! `fael vault rebuild` (hidden, issue 01M8BDGV): the open rows as markdown
//! files — one file per row, so agents `Read`/`grep` the ledger without a
//! fael call and the owner browses it in Obsidian. The jsonl log stays the
//! source of truth; the vault is derived and `rebuild` reproduces it byte for
//! byte (sorted frontmatter keys, no rebuild timestamp).
//!
//! Layout: `<vault>/<repo>/<kind>/<title-slug>-<id8>.md`, frontmatter `id
//! kind key files by from ts aliases` with `aliases: [<id8>]` so Obsidian
//! resolves `[[<id8>]]`, body = row text with cited row ids as `[[<id8>]]`.
//! Only open rows; a closed/superseded row's file is deleted. Atomic writes
//! (tmp + rename). The write path (`write::add_row`, `write::close_row_as`)
//! calls `refresh` — only when the vault dir already exists (no vault = one
//! `git config` read, then zero cost).

use crate::{Args, Repo, core};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) fn cmd(a: &Args, rest: &[String]) -> Result<(), String> {
    match rest {
        [v] if v == "rebuild" => {
            a.only("vault", &[])?;
            let r = crate::repo()?;
            let root = rebuilt(&r)?;
            println!("{root}");
            Ok(())
        }
        _ => Err("rejected: wrong arguments for \"vault\" — try 'fael vault rebuild'".into()),
    }
}

/// The vault root: `git config fael.vault` wins (shared Obsidian dir), else
/// `<git-common-dir>/fael/vault` beside the journal, so every worktree of a
/// clone shares one vault. `None` without git (then the tree is all there is).
/// The bool says the root was configured — an opt-in vault `refresh` creates
/// on first use; the default one only `rebuild` creates (no vault = zero cost).
fn vault_dir(r: &Repo) -> Option<(PathBuf, bool)> {
    if let Some(v) = crate::git(&r.root, &["config", "fael.vault"]) {
        let p = PathBuf::from(&v);
        let p = if p.is_absolute() { p } else { r.root.join(p) };
        return Some((p, true));
    }
    r.journal.as_deref().map(|j| (j.join("vault"), false))
}

/// This checkout's dir name — the vault scope, like `meta.json`'s label.
fn repo_name(root: &Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "repo".into())
}

fn id8(log: &core::Log, id: &str) -> String {
    // the row's shortest unique prefix (≥8, git-style, like every list
    // prints) — a fixed 8 chars collides for rows filed in the same
    // millisecond, which would merge their vault files into one.
    core::abbrev(log).with(id).short(id).to_string()
}

/// Filesystem slug: lowercase, runs of other chars become one `-`, empty →
/// `untitled`, cut at 60 chars on a boundary.
fn slug(s: &str) -> String {
    let slug: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug: String = slug
        .split('-')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = slug.chars().take(60).collect::<String>();
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "untitled".into()
    } else {
        slug.into()
    }
}

/// Path chars only — kinds and repo names stay readable, never escapes.
fn safe(s: &str, fallback: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let out = out.trim().trim_matches('-').to_string();
    if out.is_empty() { fallback.into() } else { out }
}

fn filename(log: &core::Log, row: &core::Row) -> String {
    let title = row.title.as_deref().unwrap_or_else(|| row.text.lines().next().unwrap_or(""));
    format!("{}-{}.md", slug(title), id8(log, &row.id))
}

fn path_for(repo_root: &Path, log: &core::Log, row: &core::Row) -> PathBuf {
    repo_root.join(safe(&row.kind, "note")).join(filename(log, row))
}

/// One JSON string — valid YAML for any scalar, byte-stable for any input.
fn q(s: &str) -> String {
    serde_json::to_string(s).expect("a string always serialises")
}

/// Replace id-shaped tokens with `[[<id8>]]`, skipping `[[…]]` spans and
/// longer alphanumeric runs (a token inside a word is not a citation).
fn splice(text: &str, tok: &str, link: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(tok) {
        let before = rest[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric());
        let after = rest[i + tok.len()..].chars().next().is_some_and(|c| c.is_alphanumeric());
        let linked = rest[..i].ends_with("[[") && rest[i + tok.len()..].starts_with("]]");
        if before || after || linked {
            out.push_str(&rest[..i + tok.len()]);
            rest = &rest[i + tok.len()..];
        } else {
            out.push_str(&rest[..i]);
            out.push_str(link);
            rest = &rest[i + tok.len()..];
        }
    }
    out.push_str(rest);
    out
}

fn linkify(log: &core::Log, text: &str, open: &HashSet<&str>) -> String {
    // one link per cited token (`id_tokens` already dedups case-insensitively);
    // only open rows link — a closed row has no vault file to point at.
    let mut pairs: Vec<(String, String)> = vec![];
    for tok in core::id_tokens(text) {
        if pairs.iter().any(|(t, _)| t == tok) {
            continue;
        }
        if let core::Ref::One(row) = core::ref_state(log, tok)
            && open.contains(row.id.as_str())
        {
            pairs.push((tok.to_string(), format!("[[{}]]", id8(log, &row.id))));
        }
    }
    pairs.sort();
    let mut out = text.to_string();
    for (tok, link) in &pairs {
        out = splice(&out, tok, link);
    }
    out
}

/// The file bytes for one open row: sorted frontmatter, then the linked body.
fn render(row: &core::Row, log: &core::Log, open: &HashSet<&str>) -> String {
    let mut fm = format!("---\naliases: [{}]\nby: {}\n", q(&id8(log, &row.id)), q(&row.by));
    if row.files.is_empty() {
        fm.push_str("files: []\n");
    } else {
        fm.push_str("files:\n");
        let mut files = row.files.clone();
        files.sort();
        for f in &files {
            fm.push_str(&format!("- {}\n", q(f)));
        }
    }
    if let Some(from) = row.from.as_deref() {
        fm.push_str(&format!("from: {}\n", q(from)));
    }
    fm.push_str(&format!("id: {}\n", q(&row.id)));
    if let Some(key) = row.key.as_deref() {
        fm.push_str(&format!("key: {}\n", q(key)));
    }
    fm.push_str(&format!("kind: {}\nts: {}\n---\n", q(&row.kind), q(&row.ts)));
    let body = linkify(log, &row.text, open);
    match body.ends_with('\n') {
        true => format!("{fm}{body}"),
        false => format!("{fm}{body}\n"),
    }
}

fn atomic_write(path: &Path, body: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("vault: {dir:?}: {e}"))?;
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("vault: {tmp:?}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("vault: {path:?}: {e}"))?;
    Ok(())
}

/// The frontmatter `id` of a vault file — `None` for foreign/broken files.
fn file_id(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let fm = s.strip_prefix("---\n")?.split("---\n").next()?;
    for line in fm.lines() {
        if let Some(v) = line.strip_prefix("id: ") {
            return serde_json::from_str::<String>(v.trim()).ok();
        }
    }
    None
}

/// Every vault file currently holding row `id` — frontmatter-verified, so a
/// filename from an older (shorter) unique prefix still matches after the
/// prefix grew, and a same-prefix collision never mismatches.
fn files_for(repo_root: &Path, id: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    let Ok(kinds) = std::fs::read_dir(repo_root) else {
        return out;
    };
    for kind in kinds.flatten() {
        let Ok(files) = std::fs::read_dir(kind.path()) else {
            continue;
        };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().is_some_and(|x| x == "md") && file_id(&p).as_deref() == Some(id) {
                out.push(p);
            }
        }
    }
    out
}

/// The log exactly as `fael find` sees it: tree + journal union, bumps
/// folded, dormant marked from this worktree's files — closed, superseded,
/// dormant, alias and carrier rows hidden from the vault.
fn open_log(r: &Repo) -> core::Log {
    let mut log = crate::read(r);
    crate::hook::mark_dormant(r, &mut log);
    log
}

/// One row's file, now: write it when the row is open, delete it when the
/// row closed (or is gone); a supersede deletes the old row's file too.
/// No-op unless the vault dir already exists.
pub(crate) fn refresh(r: &Repo, id: &str) -> Result<(), String> {
    let Some((vault, explicit)) = vault_dir(r) else {
        return Ok(());
    };
    // an opted-in vault starts on first use; the default one only `rebuild`
    // creates, so a clone that never asked for a vault pays one dir check.
    if !vault.is_dir() {
        if !explicit {
            return Ok(());
        }
        std::fs::create_dir_all(&vault).map_err(|e| format!("vault: {vault:?}: {e}"))?;
    }
    let log = open_log(r);
    let open: HashSet<&str> = core::find(&log, &core::Filter::default())
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    let root = vault.join(safe(&repo_name(&r.root), "repo"));
    match core::resolve_row(&log, id).ok() {
        Some(row) if open.contains(row.id.as_str()) => {
            let path = path_for(&root, &log, row);
            atomic_write(&path, &render(row, &log, &open))?;
            for stale in files_for(&root, &row.id) {
                if stale != path {
                    let _ = std::fs::remove_file(&stale);
                }
            }
            if let Some(old) = row.supersedes.as_deref() {
                for stale in files_for(&root, old) {
                    let _ = std::fs::remove_file(&stale);
                }
            }
        }
        _ => {
            for stale in files_for(&root, id) {
                let _ = std::fs::remove_file(&stale);
            }
        }
    }
    Ok(())
}

/// Diff the vault against the open set: write missing/changed files, delete
/// files with no open row behind them (wrong path, foreign, or closed).
/// Returns `(written, removed)`. Creates the dirs — only `rebuild` calls it.
pub(crate) fn reconcile(r: &Repo) -> Result<(usize, usize), String> {
    let Some((vault, _)) = vault_dir(r) else {
        return Err("vault: no vault dir — set git config fael.vault <dir> or run in a git repo".into());
    };
    let log = open_log(r);
    let rows = core::find(&log, &core::Filter::default());
    let open: HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let by_id: HashMap<&str, &core::Row> = rows.iter().map(|r| (r.id.as_str(), *r)).collect();
    let root = vault.join(safe(&repo_name(&r.root), "repo"));
    let (mut wrote, mut removed) = (0, 0);
    for row in &rows {
        let path = path_for(&root, &log, row);
        let body = render(row, &log, &open);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(&body) {
            atomic_write(&path, &body)?;
            wrote += 1;
        }
    }
    if let Ok(kinds) = std::fs::read_dir(&root) {
        for kind in kinds.flatten() {
            let Ok(files) = std::fs::read_dir(kind.path()) else {
                continue;
            };
            for f in files.flatten() {
                let p = f.path();
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                // `*.md.tmp` is a crashed write's litter — never a kept file
                if !name.ends_with(".md") && !name.ends_with(".md.tmp") {
                    continue;
                }
                let keep = file_id(&p)
                    .as_deref()
                    .and_then(|id| by_id.get(id))
                    .is_some_and(|row| path_for(&root, &log, row) == p);
                if !keep {
                    let _ = std::fs::remove_file(&p);
                    removed += 1;
                }
            }
            let _ = std::fs::remove_dir(kind.path());
        }
    }
    Ok((wrote, removed))
}

/// `fael vault rebuild` — the whole open set, now. Creates the vault dir.
fn rebuilt(r: &Repo) -> Result<String, String> {
    let (wrote, removed) = reconcile(r)?;
    let (vault, _) = vault_dir(r).expect("reconcile resolved it");
    let root = vault.join(safe(&repo_name(&r.root), "repo"));
    Ok(format!(
        "vault: {wrote} written, {removed} removed → {}",
        root.display()
    ))
}
