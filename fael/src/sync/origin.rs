//! The public-origin warning: `store = local` keeps rows out of the tree, yet a
//! sync that pushes `refs/fael/*` to `origin` still makes them fetchable by
//! anyone with read access. `fael.remote` may be a URL or a remote name, and one
//! repo has many spellings (`.git` suffix, ssh vs https, a `pushurl`), so both
//! sides are resolved through git and normalised before comparing.

use crate::{Repo, core};

/// One line to stderr when this sync's destination is `origin`. The push
/// happens either way.
pub(super) fn warn(r: &Repo, remote: &str) {
    if !matches!(r.cfg.store, core::Store::Local) {
        return;
    }
    // `remote get-url` resolves a remote name (and `insteadOf`); a URL is not a
    // remote, so it fails and the string itself is the destination.
    let get = |args: &[&str]| crate::git(&r.root, &[&["remote", "get-url"], args].concat());
    let dest = get(&["--push", remote]).unwrap_or_else(|| remote.to_string());
    let dest = key(&dest);
    let same = |o: Option<String>| o.is_some_and(|o| key(&o) == dest);
    if same(get(&["origin"])) || same(get(&["--push", "origin"])) {
        eprintln!(
            "fael: fael ref is publicly fetchable from origin — point fael.remote at a private remote if this repo is public"
        );
    }
}

/// `host/path` for a remote spelling, lowercase host, without scheme, user,
/// port, a trailing `/` or `.git`: `git@github.com:o/r.git`,
/// `ssh://git@github.com:22/o/r` and `https://GitHub.com/o/r/` are one repo.
/// A local path stays as it is (minus the suffix).
fn key(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u).trim_end_matches('/');
    let (host, path) = if let Some((_, rest)) = u.split_once("://") {
        let (auth, path) = rest.split_once('/').unwrap_or((rest, ""));
        (Some(auth), path)
    } else {
        // scp-like `[user@]host:path`, but not a Windows drive (`C:\x`) or a path
        match u.split_once(':') {
            Some((h, p)) if h.len() > 1 && !h.contains(['/', '\\']) => (Some(h), p),
            _ => (None, u),
        }
    };
    let Some(auth) = host else {
        return path.to_string();
    };
    let host = auth.rsplit_once('@').map_or(auth, |(_, h)| h);
    let host = host.split_once(':').map_or(host, |(h, _)| h);
    format!("{}/{}", host.to_lowercase(), path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn one_repo_many_spellings() {
        let one = "github.com/o/r";
        for u in [
            "git@github.com:o/r.git",
            "ssh://git@github.com:22/o/r",
            "https://GitHub.com/o/r/",
            "https://user:tok@github.com/o/r.git",
        ] {
            assert_eq!(key(u), one, "{u}");
        }
        assert_ne!(key("https://github.com/o/other"), one);
        // paths and drives are not hosts
        assert_eq!(key("/srv/git/r.git"), "/srv/git/r");
        assert_eq!(key(r"C:\repos\r.git"), r"C:\repos\r");
    }
}
