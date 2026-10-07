//! `find --branches` (PLAN-fael-row-hygiene chunk 9): rows on branches not
//! yet merged into HEAD, read without a checkout. The working tree never
//! moves: `for-each-ref --no-merged` lists the candidates (branches whose
//! rows HEAD already holds need no read of their own), `ls-tree` names each
//! ref's `.fael/log` blobs, and one `cat-file --batch` process streams them
//! all — parsed by `core::parse`, never by a second implementation.
//!
//! Core never spawns processes (§4), so this lives on the binary side and
//! feeds `find`/`kickoff` a merged log: working-tree rows win on duplicate
//! ids, branch-only rows render with ` @<branch>`. Read/edit push never calls
//! here (no git spawn on the 5 ms push path) — `find` and `kickoff` only.

use crate::core;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Full row id → display branch (`origin/` stripped) for rows HEAD does not
/// hold. Empty when there is no git, no other branch, or every listing fails.
pub type BranchMap = HashMap<String, String>;

/// `with_branches` on the union log, plus the journal tags, plus one line
/// when no branch added a row the union lacks — why, never a silent plain list
/// (moat-token chunk 4, S4). CLI prints it to stderr, MCP appends it.
pub fn widen(
    r: &crate::Repo,
    base: core::Log,
    journal: BranchMap,
) -> (core::Log, BranchMap, Option<&'static str>) {
    let (log, btags) = with_branches(&r.root, base);
    let note = btags.is_empty().then_some(match r.cfg.store {
        core::Store::Local => {
            "fael: --branches added no rows beyond plain find — it reads rows committed to other branches' .fael/log; \
under store = \"local\" rows live in the journal plain find already reads (this machine's branches and synced ones)"
        }
        core::Store::Tracked => {
            "fael: --branches added no rows beyond plain find — no unmerged branch commits a row \
this clone's journal lacks (or .fael/log is gitignored)"
        }
    });
    (log, crate::journal::overlay(journal, btags), note)
}

/// The working-tree `base` plus every unmerged branch's rows (HEAD wins on
/// duplicate ids), with the branch each extra row came from.
pub fn with_branches(root: &Path, base: core::Log) -> (core::Log, BranchMap) {
    let mut log = base;
    let mut branch_of = BranchMap::new();
    let mut seen: HashSet<String> = log.rows.iter().map(|r| r.id.clone()).collect();
    let mut seen_close: HashSet<String> = log.closes.iter().map(|r| r.id.clone()).collect();
    for (branch, bytes, close) in blobs(root) {
        let mut rows = vec![];
        let mut warnings = vec![];
        core::parse(&bytes, &branch, &mut rows, &mut warnings);
        let (ids, dst) = if close {
            (&mut seen_close, &mut log.closes)
        } else {
            (&mut seen, &mut log.rows)
        };
        for r in rows {
            if ids.insert(r.id.clone()) {
                branch_of.insert(r.id.clone(), branch.clone());
                dst.push(r);
            }
        }
    }
    (log, branch_of)
}

/// `(display branch, blob bytes, is_close)` for every `.fael/log` blob on
/// every unmerged branch, in `for-each-ref` (refname) order — deterministic,
/// so the first branch holding a duplicate id wins the tag.
fn blobs(root: &Path) -> Vec<(String, Vec<u8>, bool)> {
    let refs = unmerged_refs(root);
    if refs.is_empty() {
        return vec![];
    }
    // one ls-tree per ref (cheap, local); the blobs stream through one batch
    let mut wants = vec![];
    for (name, _) in &refs {
        let out = Command::new("git")
            .args(["ls-tree", "-r", "--name-only", name, "--", ".fael/log"])
            .current_dir(root)
            .output();
        let Ok(out) = out else { continue };
        if !out.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            // ls-tree --name-only prints one raw path per line; only the rows
            // travel — quarantine and caches are local state, never memory
            if line.ends_with(".jsonl") {
                wants.push((
                    display(name),
                    format!("{name}:{line}"),
                    line.ends_with(".close.jsonl"),
                ));
            }
        }
    }
    batch(&wants, root)
}

/// `ref → bytes` through a single `cat-file --batch` process. A ref deleted
/// mid-run answers `missing` and is skipped; anything unparseable is skipped.
fn batch(wants: &[(String, String, bool)], root: &Path) -> Vec<(String, Vec<u8>, bool)> {
    if wants.is_empty() {
        return vec![];
    }
    let mut child = match Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    if let Some(mut stdin) = child.stdin.take() {
        for (_, key, _) in wants {
            let _ = writeln!(stdin, "{key}");
        }
    }
    let out = child
        .wait_with_output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    // `<sha> <type> <size>\n<size bytes>\n` per hit, `<key> missing\n` per miss
    let mut blobs = vec![];
    let mut i = 0;
    let mut n = 0;
    while n < wants.len() {
        let (branch, _, close) = &wants[n];
        n += 1;
        let Some(eol) = out[i..].iter().position(|&b| b == b'\n') else {
            break;
        };
        let header = &out[i..i + eol];
        i += eol + 1;
        if header.ends_with(b"missing") {
            continue;
        }
        let Some(size) = header
            .rsplit(|&b| b == b' ')
            .next()
            .and_then(|s| std::str::from_utf8(s).ok())
            .and_then(|s| s.parse::<usize>().ok())
        else {
            break;
        };
        if out.len() < i + size {
            break;
        }
        blobs.push((branch.clone(), out[i..i + size].to_vec(), *close));
        i += size;
        // each blob is followed by exactly one newline
        i += usize::from(out.get(i) == Some(&b'\n'));
    }
    blobs
}

/// `(refname, sha)` for branches HEAD does not contain yet — merged branches'
/// rows are already in the working log, so they need no read. `origin/HEAD`
/// (a pointer, not a branch) and remote refs pointing at a local tip (a fetch
/// of our own push) are skipped. Local vs remote comes from the full refname,
/// not from a `/`: a local `feat/x` contains a slash and must still count as
/// local, or `origin/feat/x` is read a second time.
fn unmerged_refs(root: &Path) -> Vec<(String, String)> {
    let out = Command::new("git")
        .args([
            "for-each-ref",
            "--no-merged=HEAD",
            "--format=%(refname) %(refname:short) %(objectname)",
            "refs/heads",
            "refs/remotes",
        ])
        .current_dir(root)
        .output();
    let Ok(out) = out else { return vec![] };
    if !out.status.success() {
        return vec![];
    }
    let mut local_shas = HashSet::new();
    let mut refs = vec![];
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.splitn(3, ' ');
        let (full, name, sha) = match (parts.next(), parts.next(), parts.next()) {
            (Some(f), Some(n), Some(s)) if !f.is_empty() && !n.is_empty() && !s.is_empty() => {
                (f, n.to_string(), s.to_string())
            }
            _ => continue,
        };
        if name.ends_with("/HEAD") {
            continue;
        }
        let local = full.starts_with("refs/heads/");
        if local {
            local_shas.insert(sha.clone());
        }
        refs.push((local, name, sha));
    }
    refs.into_iter()
        .filter(|(local, _, sha)| *local || !local_shas.contains(sha))
        .map(|(_, name, sha)| (name, sha))
        .collect()
}

/// What the agent sees after the row: `origin/feat/x` is our fetch of
/// `feat/x`, so the remote prefix goes — a bare `feat/x` never collides here
/// (a local tip with the same sha is skipped above).
fn display(name: &str) -> String {
    name.strip_prefix("origin/").unwrap_or(name).to_string()
}

/// Tag every rendered row line that came from another branch: `- [id] … rst`
/// becomes `- [id] … rst @branch`. Cut/bodies lines never start with `- [`,
/// so they pass through untouched. No branch map (plain `find`) = identity.
pub fn tag(out: String, branch_of: &BranchMap) -> String {
    if branch_of.is_empty() {
        return out;
    }
    let mut tagged = String::with_capacity(out.len());
    for line in out.split_inclusive('\n') {
        let short = line
            .strip_prefix("- [")
            .and_then(|l| l.split(']').next())
            .filter(|s| s.len() >= 8);
        let branch = short.and_then(|s| {
            branch_of
                .iter()
                .find(|(id, _)| id.starts_with(s))
                .map(|(_, b)| b)
        });
        match branch {
            // a bare suffix with no branch behind it (`(files changed since)`)
            // rides the same channel, minus the `@` — never `@(...)`
            Some(b) => {
                let (body, nl) = line
                    .strip_suffix('\n')
                    .map(|l| (l, "\n"))
                    .unwrap_or((line, ""));
                match b.starts_with('(') {
                    true => tagged.push_str(&format!("{body} {b}{nl}")),
                    false => tagged.push_str(&format!("{body} @{b}{nl}")),
                }
            }
            None => tagged.push_str(line),
        }
    }
    tagged
}
