//! The checkout a command runs in: root, `.fael` dir, config and the clone-shared
//! journal; plus the thin git/read helpers every client (fael, fael-crew) shares.

use crate::{Config, Log};
use std::path::{Path, PathBuf};

pub struct Repo {
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub fael: PathBuf,
    pub cfg: Config,
    /// Clone-shared journal (`<git-common-dir>/fael`); `None` without git.
    pub journal: Option<PathBuf>,
}

pub fn repo() -> Result<Repo, String> {
    let cwd = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| format!("cwd: {e}"))?;
    repo_at(&cwd)
}

/// The same resolution from an explicit directory — what hooks pass.
pub fn repo_at(cwd: &Path) -> Result<Repo, String> {
    // normalize_files is lexical, so root must be symlink-resolved like cwd
    let cwd = cwd.canonicalize().map_err(|e| format!("cwd: {e}"))?;
    // ponytail: walk up for `.git` (dir, or file for worktree/submodule) instead
    // of spawning `git rev-parse` — that spawn was ~13 of a hook's ~15 ms.
    // Ignores GIT_DIR/GIT_WORK_TREE/core.worktree; spawn git if those matter.
    let find = |name: &str| {
        cwd.ancestors()
            .find(|d| d.join(name).exists())
            .map(Path::to_path_buf)
    };
    let root = find(".git")
        .or_else(|| find(".fael"))
        .unwrap_or_else(|| cwd.clone());
    // FAEL_DIR: scratch log (tree only, no journal) — see CONTRIBUTING.
    let scratch = std::env::var_os("FAEL_DIR").filter(|d| !d.is_empty());
    let journal = scratch
        .is_none()
        .then(|| crate::journal::root(&root))
        .flatten();
    let fael = scratch.map_or_else(|| root.join(".fael"), PathBuf::from);
    let cfg = config(&fael.join("config.toml"))?;
    Ok(Repo {
        root,
        cwd,
        fael,
        cfg,
        journal,
    })
}

/// `git <args>` in `dir`: trimmed stdout when it exits 0 and prints something.
pub fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let (code, out) = git_output(dir, args)?;
    let s = out.trim().to_string();
    (code == Some(0) && !s.is_empty()).then_some(s)
}

/// `git <args>` in `dir`: its exit code and stdout; `None` when git cannot spawn.
pub fn git_output(dir: &Path, args: &[&str]) -> Option<(Option<i32>, String)> {
    let o = crate::gitcmd::git_cmd()
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    Some((
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    ))
}

/// `.fael/config.toml` — missing file = defaults, a broken file = error.
fn config(path: &Path) -> Result<Config, String> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return Ok(Config::default());
    };
    Config::from_toml(&s).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read the log — tree + journal union, bumps folded; skipped lines go to
/// stderr, never fail the command.
pub fn read(r: &Repo) -> Log {
    crate::fold_bumps(crate::journal::merged(r))
}

/// A same-directory file on disk close enough that `bad` is almost surely a
/// typo of it — `None` when the parent directory is unreadable or nothing is
/// within distance 2. Only siblings count: a row file one char away may be a
/// genuinely new file (`src/a.rs` exists, `src/b.rs` is planned work), and
/// rejecting that is the false block the plan forbids.
pub fn sibling_suggest(root: &Path, bad: &str) -> Option<String> {
    let dir = bad.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let sib = if dir.is_empty() {
        root.to_path_buf()
    } else {
        root.join(dir)
    };
    let Ok(rd) = std::fs::read_dir(&sib) else {
        return None;
    };
    let mut best: Option<(usize, String)> = None;
    for e in rd.flatten().take(500) {
        let n = e.file_name().to_string_lossy().replace('\\', "/");
        let cand = if dir.is_empty() {
            n
        } else {
            format!("{dir}/{n}")
        };
        if cand == bad {
            continue;
        }
        let d = crate::levenshtein(bad, &cand);
        if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            best = Some((d, cand));
        }
    }
    let (d, cand) = best?;
    (d <= 2 && d < bad.chars().count()).then_some(cand)
}
