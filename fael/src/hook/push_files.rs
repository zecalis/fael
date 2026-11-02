//! The push's file resolution: client paths to repo-relative files, with the
//! sibling-worktree fallback. Split out of `push.rs` (file-size ratchet).

use super::protocol::Ctx;
use crate::core;
use fael_core::is_anchor;

/// The pushed paths as repo-relative files. Outside the repo falls away,
/// never errors out.
pub(super) fn repo_files(c: &Ctx, raw: &[String]) -> Vec<String> {
    let mut files = vec![];
    for f in raw {
        // the client sends whatever the OS gave it (`/var/…` vs `/private/var/…`);
        // resolve symlinks while the repo root is already resolved, or the
        // lexical strip in normalize_files reads the file as outside the repo
        // (anchors are opaque, never filesystem paths)
        let f = if is_anchor(f) {
            f.clone()
        } else {
            std::fs::canonicalize(c.repo.cwd.join(f)) // join keeps an absolute f
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| f.clone())
        };
        let one = std::slice::from_ref(&f);
        if let Ok(mut n) = core::normalize_files(one, &c.repo.cwd, &c.repo.root)
            .or_else(|e| sibling(c, &f).map_or(Err(e), |r| core::normalize_files(one, &r, &r)))
        {
            files.append(&mut n);
        }
    }
    files
}

/// The root of the sibling worktree holding `f` when it shares this clone's
/// journal: a session that `cd`s into `git worktree add ../wt` reads and edits
/// the same repo files there.
fn sibling(c: &Ctx, f: &str) -> Option<std::path::PathBuf> {
    let common = |root: &std::path::Path| crate::journal::root(root)?.parent()?.canonicalize().ok();
    let mine = common(&c.repo.root)?;
    let root = std::path::Path::new(f)
        .ancestors()
        .skip(1)
        .find(|d| d.join(".git").exists())?;
    (common(root)? == mine).then(|| root.to_path_buf())
}
