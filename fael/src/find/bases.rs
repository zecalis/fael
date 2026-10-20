//! `git merge-base HEAD <tip>` for many tips at once. It is a pure function of
//! object ids, so `cache/merge-bases` (see `flat_cache`) keeps every answer under
//! `<head>:<tip>`; the tips it has not seen share one walk instead of a spawn each
//! (decision 01M5CRPS): `rev-list --parents --boundary <tips> ^HEAD` holds every
//! commit the tips reach that HEAD does not, and the boundary commits their walks
//! end on are the common ancestors. One boundary commit is the merge base — every
//! best common ancestor is among them, so a lone one is it. None means no
//! common ancestor; several, git picks (the one case that still spawns per tip).

use super::flat_cache::{Cache, NONE};
use super::merged::{git, git_out};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Each tip's merge base with `head`; `None` for unrelated histories and for
/// whatever git failed to say (that one is not cached).
pub(super) fn merge_bases(
    root: &Path,
    head: &str,
    tips: &[&str],
) -> HashMap<String, Option<String>> {
    let mut cache = Cache::load(root, "merge-bases");
    let mut out = HashMap::new();
    let mut miss: Vec<&str> = vec![];
    for t in tips {
        match cache.map.get(&format!("{head}:{t}")) {
            Some(v) => {
                out.insert(t.to_string(), (v != NONE).then(|| v.clone()));
            }
            None if !miss.contains(t) => miss.push(t),
            None => {}
        }
    }
    if miss.is_empty() {
        return out;
    }
    let walked = walk(root, head, &miss).unwrap_or_default();
    let mut put = vec![];
    for t in miss {
        let found = walked.get(t).cloned().or_else(|| single(root, head, t));
        if let Some(base) = &found {
            put.push((
                format!("{head}:{t}"),
                base.clone().unwrap_or_else(|| NONE.to_string()),
            ));
        }
        out.insert(t.to_string(), found.flatten());
    }
    cache.put(put);
    out
}

/// `Some(None)`: git says the histories share nothing. `None`: git failed.
fn single(root: &Path, head: &str, tip: &str) -> Option<Option<String>> {
    let (code, out) = git_out(root, &["merge-base", head, tip])?;
    let base = out.trim();
    match code {
        Some(0) if !base.is_empty() => Some(Some(base.to_string())),
        Some(1) if base.is_empty() => Some(None),
        _ => None,
    }
}

fn walk<'a>(root: &Path, head: &str, tips: &[&'a str]) -> Option<HashMap<&'a str, Option<String>>> {
    let not_head = format!("^{head}");
    let mut args = vec!["rev-list", "--parents", "--boundary"];
    args.extend(tips);
    args.push(&not_head);
    let list = git(root, &args)?;
    // the commits the tips reach and HEAD does not, with their parents; the
    // boundary lines (`-<sha>`) are HEAD's side and not needed to tell them apart
    let graph: HashMap<&str, Vec<&str>> = list
        .lines()
        .filter(|l| !l.starts_with('-'))
        .filter_map(|l| {
            let mut f = l.split(' ');
            Some((f.next()?, f.collect()))
        })
        .collect();
    let mut out = HashMap::new();
    for &t in tips {
        if !graph.contains_key(t) {
            // HEAD reaches it: it is its own merge base
            out.insert(t, Some(t.to_string()));
            continue;
        }
        let (mut seen, mut todo): (HashSet<&str>, Vec<&str>) = (HashSet::new(), vec![t]);
        let mut ends: HashSet<&str> = HashSet::new();
        while let Some(c) = todo.pop() {
            if !seen.insert(c) {
                continue;
            }
            for p in &graph[c] {
                if graph.contains_key(p) {
                    todo.push(p);
                } else {
                    ends.insert(p);
                }
            }
        }
        match ends.len() {
            0 => out.insert(t, None),
            1 => out.insert(t, ends.iter().next().map(|b| b.to_string())),
            _ => None,
        };
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn g(d: &Path, args: &[&str]) -> String {
        let o = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(d)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn commit(d: &Path, msg: &str) -> String {
        g(d, &["commit", "-q", "--allow-empty", "-m", msg]);
        g(d, &["rev-parse", "HEAD"])
    }

    /// One walk answers as `git merge-base HEAD <tip>` does for tips of every
    /// kind: behind HEAD, level with it, branched off it, merged into it, a
    /// criss-cross (two best bases — git picks), and an unrelated root.
    #[test]
    fn one_walk_gives_the_merge_bases_git_gives() {
        let d = std::env::temp_dir().join(format!("fael-bases-{}", fael_core::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        g(&d, &["init", "-q", "-b", "main"]);
        let root = commit(&d, "root");
        let behind = commit(&d, "behind");
        g(&d, &["switch", "-qc", "x", "main"]);
        let x = commit(&d, "x");
        g(&d, &["switch", "-qc", "y", &behind]);
        let y = commit(&d, "y");
        // criss-cross: x and y merged into each other
        g(&d, &["switch", "-q", "x"]);
        g(&d, &["merge", "-q", "--no-ff", "-m", "x<-y", "y"]);
        let xy = g(&d, &["rev-parse", "HEAD"]);
        g(&d, &["switch", "-q", "y"]);
        g(&d, &["merge", "-q", "--no-ff", "-m", "y<-x", &x]);
        let yx = g(&d, &["rev-parse", "HEAD"]);
        g(&d, &["switch", "-q", "main"]);
        commit(&d, "main moves");
        g(&d, &["switch", "-q", "--orphan", "other"]);
        let other = commit(&d, "unrelated");
        g(&d, &["switch", "-q", "main"]);
        g(&d, &["merge", "-q", "--no-ff", "-m", "merge y", &y]);
        commit(&d, "after");
        let head = g(&d, &["rev-parse", "HEAD"]);
        let tips = [&root, &behind, &head, &x, &y, &xy, &yx, &other];
        let tips: Vec<&str> = tips.iter().map(|s| s.as_str()).collect();
        let got = merge_bases(&d, &head, &tips);
        for t in tips {
            let (code, out) = git_out(&d, &["merge-base", &head, t]).unwrap();
            let want = (code == Some(0)).then(|| out.trim().to_string());
            assert_eq!(got[t], want, "tip {t}");
        }
        // the answers are cached: a second ask spawns nothing
        let again = merge_bases(&d, &head, &[&x, &other]);
        assert_eq!(again[x.as_str()], got[x.as_str()]);
    }
}
