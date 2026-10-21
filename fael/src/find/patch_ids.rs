//! Patch-ids for the squash-merge proof (`view`), cached on disk. A commit's
//! patch-id and a `base..tip` diff's never change, so `cache/patch-ids` (see
//! `flat_cache`) keeps them for good: key a commit sha or `<base>..<tip>`,
//! value the id, `-` for a commit with no patch (fael:01M5CPNJ).

use super::flat_cache::{Cache, NONE};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::process::{Child, Stdio};

const FILE: &str = "patch-ids";
const SQUASH: &str = "squash";

/// The patch-id of `git diff <base> <tip>`, `None` when it has none or git failed.
pub(super) fn diff_id(root: &Path, base: &str, tip: &str) -> Option<String> {
    let key = format!("{base}..{tip}");
    let mut cache = Cache::load(root, FILE);
    if let Some(v) = cache.map.get(&key) {
        return (v != NONE).then(|| v.clone());
    }
    let src = spawn(root, &["diff", base, tip], false)?;
    let id = pipe_ids(root, src)?.pop().map(|(p, _)| p);
    cache.put(vec![(key, id.clone().unwrap_or_else(|| NONE.into()))]);
    id
}

/// The squash verdicts of `tips` against `head`, kept in `cache/squash` (key
/// `<head>:<tip>`, value `1`/`0`): whether the tip's `base..tip` diff has the
/// patch-id of a commit on HEAD is a pure function of the two object ids, and
/// proving it costs the `commit_lists` walk on every pull. Only a proven
/// answer is stored, never a git failure.
pub(super) fn squash_cached(root: &Path, head: &str, tips: &[&str]) -> HashMap<String, bool> {
    let cache = Cache::load(root, SQUASH);
    tips.iter()
        .filter_map(|t| {
            let v = cache.map.get(&format!("{head}:{t}"))?;
            Some((t.to_string(), v == "1"))
        })
        .collect()
}

pub(super) fn squash_put(root: &Path, head: &str, verdicts: &[(&str, bool)]) {
    let put = verdicts
        .iter()
        .map(|(t, v)| {
            (
                format!("{head}:{t}"),
                if *v { "1" } else { "0" }.to_string(),
            )
        })
        .collect();
    Cache::load(root, SQUASH).put(put);
}

/// For each base, the patch-ids of the non-merge commits of `base..head` — what
/// `git log -p --no-merges base..HEAD | git patch-id` gives, one walk for all
/// the bases. Only the commits the cache has not seen are diffed, in one
/// `log -p --stdin`. A base git could not list has no entry.
pub(super) fn head_ids(
    root: &Path,
    head: &str,
    bases: &[String],
) -> HashMap<String, HashSet<String>> {
    let lists = commit_lists(root, head, bases);
    let mut cache = Cache::load(root, FILE);
    let new: Vec<&str> = lists
        .values()
        .flatten()
        .map(String::as_str)
        .filter(|c| !cache.map.contains_key(*c))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if !new.is_empty() {
        // `log` rather than `diff-tree`: same rename and format config as the
        // range walk, so the ids are the ones `log -p base..HEAD` gives
        if let Some(found) = log_ids(root, &new) {
            let mut put: Vec<(String, String)> = new
                .iter()
                .map(|c| (c.to_string(), NONE.to_string()))
                .collect();
            put.extend(found.into_iter().map(|(p, c)| (c, p)));
            // a later entry wins on read, so the found id overrides its `-`
            cache.put(put);
        }
    }
    lists
        .into_iter()
        .map(|(b, commits)| {
            let ids = commits
                .iter()
                .filter_map(|c| cache.map.get(c))
                .filter(|v| *v != NONE)
                .cloned()
                .collect();
            (b, ids)
        })
        .collect()
}

/// The non-merge commits of `base..head`, per base. One base is a plain
/// `rev-list`. Several share one walk: HEAD's history above their common
/// ancestor `m` (`merge-base --octopus`), with parents, holds every one of the
/// ranges — each is that history minus what the base reaches inside it.
fn commit_lists(root: &Path, head: &str, bases: &[String]) -> HashMap<String, Vec<String>> {
    let one = |b: &String| {
        let list = super::merged::git(root, &["rev-list", "--no-merges", &format!("{b}..{head}")])?;
        Some((b.clone(), list.lines().map(str::to_string).collect()))
    };
    if let [b] = bases {
        return one(b).into_iter().collect();
    }
    let walk = || {
        let mut args = vec!["merge-base", "--octopus"];
        args.extend(bases.iter().map(String::as_str));
        let m = super::merged::git(root, &args)?;
        let walk = super::merged::git(root, &["rev-list", "--parents", head, &format!("^{m}")])?;
        let graph: HashMap<&str, Vec<&str>> = walk
            .lines()
            .filter_map(|l| {
                let mut f = l.split(' ');
                Some((f.next()?, f.collect()))
            })
            .collect();
        Some(
            bases
                .iter()
                .map(|b| {
                    let mut seen: HashSet<&str> = HashSet::new();
                    let mut todo = vec![b.as_str()];
                    while let Some(c) = todo.pop() {
                        if let Some(ps) = graph.get(c).filter(|_| seen.insert(c)) {
                            todo.extend(ps);
                        }
                    }
                    let mut own: Vec<String> = graph
                        .iter()
                        .filter(|(c, ps)| ps.len() < 2 && !seen.contains(*c))
                        .map(|(c, _)| c.to_string())
                        .collect();
                    own.sort();
                    (b.clone(), own)
                })
                .collect(),
        )
    };
    // histories with no common ancestor: the plain range per base
    walk().unwrap_or_else(|| bases.iter().filter_map(one).collect())
}

fn log_ids(root: &Path, shas: &[&str]) -> Option<Vec<(String, String)>> {
    let mut src = spawn(
        root,
        &["log", "-p", "--no-merges", "--no-walk=unsorted", "--stdin"],
        true,
    )?;
    // git reads every rev before it walks, so writing them all first cannot stall
    let mut w = src.stdin.take()?;
    w.write_all(shas.join("\n").as_bytes()).ok()?;
    drop(w);
    pipe_ids(root, src)
}

fn spawn(root: &Path, args: &[&str], stdin: bool) -> Option<Child> {
    #[cfg(test)]
    super::merged::SPAWNED.with(|s| s.borrow_mut().push(args.join(" ")));
    fael_core::git_cmd()
        .args(args)
        .current_dir(root)
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// `src | git patch-id --stable` as (patch-id, commit) pairs; `None` when either
/// side failed. A real pipe, so neither side buffers a whole `log -p` in memory.
fn pipe_ids(root: &Path, mut src: Child) -> Option<Vec<(String, String)>> {
    let pipe = src.stdout.take()?;
    let out = fael_core::git_cmd()
        .args(["patch-id", "--stable"])
        .current_dir(root)
        .stdin(pipe)
        .stderr(Stdio::null())
        .output();
    let src_ok = src.wait().is_ok_and(|s| s.success());
    let out = out.ok().filter(|o| o.status.success() && src_ok)?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect(),
    )
}
