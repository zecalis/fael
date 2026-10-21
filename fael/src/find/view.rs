//! The git facts a pull needs to say whether a branch is merged into HEAD, read
//! once however many branches the repo holds (decision 01M5CRPS). Every spawn
//! here costs a fixed number, never one per branch:
//!
//! * one `for-each-ref` lists every ref — a branch name resolves in the map the
//!   way `rev-parse` resolves it, no spawn per branch;
//! * one `for-each-ref --merged HEAD` is the ancestor check of every branch tip;
//! * one `rev-list --first-parent HEAD`: a branch on that line never left the
//!   commit it was cut from — not "merged";
//! * `merge-base HEAD <tip>` is a pure function of object ids, kept in
//!   `cache/merge-bases`; the tips it has not seen share one walk (`bases`);
//! * the squash proof's `base..HEAD` patch-ids come from one walk (`patch_ids`).
//!
//! What depends on HEAD or on where the refs point lives in this view, which
//! one pull owns: `forget` ends it (the MCP server calls it per tool call; a
//! CLI process is one pull). Unproven = not merged, and a branch name the map
//! cannot resolve exactly as git would (a pseudo-ref, a hex-like name) is
//! asked of `rev-parse` itself.

use super::merged::git;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

enum Ref {
    Commit(String),
    /// A tag of a tree or a blob: `^{commit}` fails on it.
    Other,
    /// A tag of a tag: peeling it is git's to do.
    Nested,
}

pub(super) struct View {
    root: PathBuf,
    common: Option<PathBuf>,
    /// `None`: no commit on HEAD — nothing can be proved.
    head: Option<String>,
    /// Every ref by full name; `None` when `for-each-ref` failed (git decides each name).
    refs: Option<HashMap<String, Ref>>,
    /// Tips of `refs/heads` + `refs/remotes`, and which of them HEAD contains.
    tips: HashSet<String>,
    merged: HashSet<String>,
    first_parent: Option<Option<HashSet<String>>>,
    verdicts: HashMap<(String, bool), bool>,
}

thread_local!(static VIEW: RefCell<Option<View>> = const { RefCell::new(None) });

/// End the pull: the next question reads the refs again.
pub(crate) fn forget() {
    VIEW.with(|v| *v.borrow_mut() = None);
}

/// For each `(branch, stamped tip sha)`: does git prove it merged into HEAD? The
/// live ref wins; with none, the sha if its commit is still in the object store —
/// by patch-id only (the stamped tip may predate later commits on the branch, so
/// "is an ancestor of HEAD" would prove too much). Asking nothing spawns nothing.
pub(crate) fn prove(root: &Path, items: &[(&str, Option<&str>)]) -> Vec<bool> {
    if items.is_empty() {
        return vec![];
    }
    VIEW.with(|cell| {
        let mut cell = cell.borrow_mut();
        let head = crate::journal::head_sha(root);
        // HEAD moved under the view (a test committing, a checkout): read again
        if cell
            .as_ref()
            .is_none_or(|v| v.root != root || (head.is_some() && v.head != head))
        {
            *cell = Some(View::load(root));
        }
        cell.as_mut().map(|v| v.prove(items)).unwrap_or_default()
    })
}

impl View {
    fn load(root: &Path) -> Self {
        let head = crate::journal::head_sha(root)
            .or_else(|| git(root, &["rev-parse", "--verify", "-q", "HEAD^{commit}"]));
        let mut v = View {
            root: root.to_path_buf(),
            common: crate::journal::root(root).and_then(|j| j.parent().map(Path::to_path_buf)),
            head,
            refs: None,
            tips: HashSet::new(),
            merged: HashSet::new(),
            first_parent: None,
            verdicts: HashMap::new(),
        };
        if v.head.is_none() {
            return v;
        }
        v.refs = git(
            root,
            &[
                "for-each-ref",
                "--format=%(objectname) %(objecttype) %(*objectname) %(*objecttype) %(refname)",
            ],
        )
        .map(|list| {
            let mut refs = HashMap::new();
            for l in list.lines() {
                let f: Vec<&str> = l.split(' ').collect();
                let &[oid, ty, peeled, peeled_ty, name] = &f[..] else {
                    continue;
                };
                let r = match (ty, peeled_ty) {
                    ("commit", _) => Ref::Commit(oid.to_string()),
                    ("tag", "commit") => Ref::Commit(peeled.to_string()),
                    ("tag", "tag") => Ref::Nested,
                    _ => Ref::Other,
                };
                if let Ref::Commit(c) = &r
                    && (name.starts_with("refs/heads/") || name.starts_with("refs/remotes/"))
                {
                    v.tips.insert(c.clone());
                }
                refs.insert(name.to_string(), r);
            }
            refs
        });
        v.merged = git(
            root,
            &[
                "for-each-ref",
                "--merged",
                "HEAD",
                "--format=%(objectname)",
                "refs/heads",
                "refs/remotes",
            ],
        )
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
        v
    }

    fn prove(&mut self, items: &[(&str, Option<&str>)]) -> Vec<bool> {
        let Some(head) = self.head.clone() else {
            return vec![false; items.len()];
        };
        // the live ref, else the stamped sha (resolved together, in one spawn)
        let mut resolved: Vec<Option<(String, bool)>> = items
            .iter()
            .map(|(b, _)| self.live_tip(b).map(|t| (t, true)))
            .collect();
        let stamped: Vec<&str> = items
            .iter()
            .zip(&resolved)
            .filter(|(_, t)| t.is_none())
            .filter_map(|((_, sha), _)| *sha)
            .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_hexdigit()))
            .collect();
        let found = commits_of(&self.root, &stamped);
        for ((_, sha), t) in items.iter().zip(&mut resolved) {
            if t.is_none() {
                *t = sha.and_then(|s| found.get(s)).map(|c| (c.clone(), false));
            }
        }
        // a tip HEAD contains is decided by ancestry alone; the rest need the squash proof.
        // A branch or remote tip is in the `--merged` listing or not; any other (a tag,
        // a stamped sha) is HEAD's ancestor when it is its own merge base with HEAD.
        let wanted: HashSet<&str> = resolved
            .iter()
            .flatten()
            .map(|(tip, _)| tip.as_str())
            .filter(|t| !(self.tips.contains(*t) && self.merged.contains(*t)))
            .collect();
        let wanted: Vec<&str> = wanted.into_iter().collect();
        let bases = if wanted.is_empty() {
            HashMap::new()
        } else {
            super::bases::merge_bases(&self.root, &head, &wanted)
        };
        let mut pending: Vec<(String, String, String)> = vec![];
        // squash verdicts proven on an earlier pull at this HEAD (object ids only)
        let known = super::patch_ids::squash_cached(&self.root, &head, &wanted);
        let mut proven: Vec<(String, bool)> = vec![];
        for (tip, anc) in resolved.iter().flatten() {
            if self.verdicts.contains_key(&(tip.clone(), *anc)) {
                continue;
            }
            if let Some(v) = known.get(tip) {
                self.verdicts.insert((tip.clone(), *anc), *v);
                continue;
            }
            let base = bases.get(tip).cloned().flatten();
            let contained = if self.tips.contains(tip) {
                self.merged.contains(tip)
            } else {
                base.as_deref() == Some(tip)
            };
            if contained {
                let v = *anc && self.first_parent().is_some_and(|m| !m.contains(tip));
                self.verdicts.insert((tip.clone(), *anc), v);
                continue;
            }
            let want = base
                .as_ref()
                .and_then(|b| super::patch_ids::diff_id(&self.root, b, tip));
            match (base, want) {
                (Some(b), Some(w)) => pending.push((tip.to_string(), b, w)),
                _ => {
                    self.verdicts.insert((tip.to_string(), *anc), false);
                }
            }
        }
        if !pending.is_empty() {
            let mut bs: Vec<String> = pending.iter().map(|(_, b, _)| b.clone()).collect();
            bs.sort();
            bs.dedup();
            let ids = super::patch_ids::head_ids(&self.root, &head, &bs);
            for (tip, base, want) in pending {
                // a base git could not list has no entry: a failure, not a verdict
                let listed = ids.contains_key(&base);
                let v = ids.get(&base).is_some_and(|s| s.contains(&want));
                for anc in [true, false] {
                    self.verdicts.insert((tip.clone(), anc), v);
                }
                if listed {
                    proven.push((tip, v));
                }
            }
        }
        if !proven.is_empty() {
            let p: Vec<(&str, bool)> = proven.iter().map(|(t, v)| (t.as_str(), *v)).collect();
            super::patch_ids::squash_put(&self.root, &head, &p);
        }
        resolved
            .iter()
            .map(|t| {
                t.as_ref().is_some_and(|(tip, anc)| {
                    self.verdicts.get(&(tip.clone(), *anc)) == Some(&true)
                })
            })
            .collect()
    }

    /// The commit `b` names as a branch: the local ref, else our fetch of it
    /// (`origin/<b>`) — `rev-parse --verify <p><b>^{commit}`, answered from the ref map.
    fn live_tip(&self, b: &str) -> Option<String> {
        ["", "origin/"].iter().find_map(|p| {
            let name = format!("{p}{b}");
            match self.lookup(&name) {
                Some(Some(Ref::Commit(c))) => Some(c.clone()),
                Some(Some(Ref::Other)) | Some(None) => None,
                Some(Some(Ref::Nested)) | None => git(
                    &self.root,
                    &["rev-parse", "--verify", "-q", &format!("{name}^{{commit}}")],
                ),
            }
        })
    }

    /// `Some(hit)` when the map answers as git's lookup order would —
    /// `<n>`, `refs/<n>`, `refs/tags/<n>`, `refs/heads/<n>`, `refs/remotes/<n>`,
    /// `refs/remotes/<n>/HEAD` — `None` when it cannot: ask git.
    fn lookup(&self, name: &str) -> Option<Option<&Ref>> {
        let refs = self.refs.as_ref()?;
        if !self.plain(name) {
            return None;
        }
        [
            name.to_string(),
            format!("refs/{name}"),
            format!("refs/tags/{name}"),
            format!("refs/heads/{name}"),
            format!("refs/remotes/{name}"),
            format!("refs/remotes/{name}/HEAD"),
        ]
        .iter()
        .find_map(|n| refs.get(n))
        .map(Some)
        .or(Some(None))
    }

    /// A name only refs can answer to: a hex run may be an abbreviated sha, an
    /// upper-case word a pseudo-ref (`HEAD`, `FETCH_HEAD`), a file in the git
    /// dir `$GIT_DIR/<name>`, and anything with a revision operator is no ref.
    fn plain(&self, name: &str) -> bool {
        let ok = name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._/-".contains(&c));
        let hex = (4..=64).contains(&name.len()) && name.bytes().all(|c| c.is_ascii_hexdigit());
        let pseudo = name.bytes().all(|c| c.is_ascii_uppercase() || c == b'_');
        let in_git_dir = self.common.as_ref().is_none_or(|c| c.join(name).is_file());
        ok && !hex
            && !pseudo
            && !in_git_dir
            && !name.starts_with(['-', '/'])
            && !name.contains("..")
    }

    fn first_parent(&mut self) -> Option<&HashSet<String>> {
        let root = &self.root;
        self.first_parent
            .get_or_insert_with(|| {
                git(root, &["rev-list", "--first-parent", "HEAD"])
                    .map(|l| l.lines().map(str::to_string).collect())
            })
            .as_ref()
    }
}

/// The commit each of `shas` names, in one `cat-file --batch-check`; a sha
/// that is no commit in the object store has no entry.
fn commits_of(root: &Path, shas: &[&str]) -> HashMap<String, String> {
    use std::io::Write;
    use std::process::Stdio;
    if shas.is_empty() {
        return HashMap::new();
    }
    #[cfg(test)]
    super::merged::SPAWNED.with(|s| s.borrow_mut().push("cat-file --batch-check".into()));
    let Ok(mut child) = fael_core::git_cmd()
        .args(["cat-file", "--batch-check"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return HashMap::new();
    };
    let input: String = shas.iter().map(|s| format!("{s}^{{commit}}\n")).collect();
    let mut w = child.stdin.take();
    // a thread feeds git while this one drains it: neither pipe can fill and stall
    let out = std::thread::scope(|sc| {
        sc.spawn(move || {
            if let Some(w) = w.as_mut() {
                let _ = w.write_all(input.as_bytes());
            }
        });
        child.wait_with_output()
    });
    let Ok(out) = out else { return HashMap::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .zip(shas)
        .filter_map(|(l, sha)| {
            let mut f = l.split(' ');
            let oid = f.next()?;
            (f.next() == Some("commit")).then(|| (sha.to_string(), oid.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests;
