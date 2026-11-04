//! `fael chunk status`: real textual conflicts between this chunk's worktree and every other
//! live run's, uncommitted work included (issue 01M6KHWFBT0K8RYEX73S37KTRJ). Each worktree is
//! snapshotted through a temp copy of its index (the real one is never touched), then
//! `merge-tree` compares the two snapshots. Informs only: any failure reads "could not check".
// ponytail: same machine, half-saved files may show, no semantic conflicts, each snapshot
// leaves dangling objects for `git gc`.

use fael_crew::StatusSide;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// One neighbour that is not clean.
pub(super) enum Hit {
    Conflict(StatusSide, Vec<String>),
    Unchecked(StatusSide),
}

fn run(dir: &Path, index: Option<&Path>, args: &[&str]) -> Option<String> {
    let mut c = fael_core::git_cmd();
    c.args(args).current_dir(dir);
    if let Some(i) = index {
        c.env("GIT_INDEX_FILE", i);
    }
    let o = c.output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// A commit holding `dir`'s working state (tracked, staged and untracked), parent HEAD.
fn snapshot(dir: &Path) -> Option<String> {
    let real = PathBuf::from(run(
        dir,
        None,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?);
    let tmp = std::env::temp_dir().join(format!("fael-snap-{}", fael_core::ulid()));
    if real.is_file() {
        std::fs::copy(&real, &tmp).ok()?;
    }
    let made = (|| {
        run(dir, Some(&tmp), &["add", "-A"])?;
        let tree = run(dir, Some(&tmp), &["write-tree"])?;
        let ident = ["-c", "user.name=fael", "-c", "user.email=fael@localhost"];
        let a = [
            &ident[..],
            &["commit-tree", &tree, "-p", "HEAD", "-m", "snap"],
        ]
        .concat();
        run(dir, None, &a)
    })();
    let _ = std::fs::remove_file(&tmp);
    made
}

/// Conflicts of `mine` against each neighbour; clean ones are left out.
pub(super) fn check(mine: &Path, neighbours: Vec<StatusSide>) -> Vec<Hit> {
    let mut seen: Vec<String> = vec![mine.display().to_string()];
    let mut me: Option<Option<String>> = None;
    let mut out = Vec::new();
    for n in neighbours {
        let wt = n.worktree.clone().unwrap_or_default();
        if !wt.is_empty() && seen.contains(&wt) {
            continue; // one worktree, one check
        }
        seen.push(wt.clone());
        let theirs = (!wt.is_empty() && Path::new(&wt).is_dir())
            .then(|| snapshot(Path::new(&wt)))
            .flatten();
        let mine_c = me.get_or_insert_with(|| snapshot(mine)).clone();
        let files = match (mine_c, theirs) {
            (Some(a), Some(b)) => crate::board::prs::merge_tree(mine, &a, &b),
            _ => None,
        };
        match files {
            None => out.push(Hit::Unchecked(n)),
            Some(f) if f.is_empty() => {}
            Some(f) => out.push(Hit::Conflict(n, f)),
        }
    }
    out
}

fn note(uid: &str, files: &[String]) -> String {
    format!(
        "fael chunk note {uid} \"I also edit {}: coordinate before merging\"",
        files.join(", ")
    )
}

pub(super) fn json_of(hits: &[Hit]) -> Value {
    let rows = |conflict: bool| -> Vec<Value> {
        hits.iter()
            .filter_map(|h| match h {
                Hit::Conflict(n, f) if conflict => Some(json!({
                    "uid": n.uid, "title": n.title, "worktree": n.worktree, "branch": n.branch,
                    "files": f,
                    "diff": f.iter().map(|x| format!("git -C {} diff -- {x}", n.worktree.as_deref().unwrap_or("."))).collect::<Vec<_>>(),
                    "note": note(&n.uid, f),
                })),
                Hit::Unchecked(n) if !conflict => {
                    Some(json!({"uid": n.uid, "title": n.title, "worktree": n.worktree}))
                }
                _ => None,
            })
            .collect()
    };
    json!({ "conflicts": rows(true), "unchecked": rows(false) })
}

pub(super) fn text_of(hits: &[Hit]) -> String {
    let mut o = String::new();
    for h in hits {
        match h {
            Hit::Conflict(n, f) => {
                let wt = n.worktree.as_deref().unwrap_or(".");
                let br = n.branch.as_deref().unwrap_or("-");
                o += &format!(
                    "conflict with {} \"{}\" branch {br} worktree {wt}\n",
                    n.uid, n.title
                );
                for x in f {
                    o += &format!("  {x}\n    git -C {wt} diff -- {x}\n");
                }
                o += &format!("  {}\n", note(&n.uid, f));
            }
            Hit::Unchecked(n) => {
                o += &format!("could not check {} \"{}\"\n", n.uid, n.title);
            }
        }
    }
    o
}
