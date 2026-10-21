//! The `git` every client spawns.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// `Command` for git. On macOS `/usr/bin/git` is an xcrun shim that costs ~5 ms per
/// spawn, so when it is the git on PATH we run the real binary under the developer
/// dir. Resolved once per process, never cached on disk: an Xcode update or an
/// `xcode-select` switch cannot leave a stale path.
pub fn git_cmd() -> Command {
    static GIT: OnceLock<PathBuf> = OnceLock::new();
    Command::new(GIT.get_or_init(resolve))
}

#[cfg(target_os = "macos")]
fn resolve() -> PathBuf {
    let dev = std::env::var_os("DEVELOPER_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    let link = std::fs::read_link("/var/db/xcode_select_link").ok();
    let path = std::env::var_os("PATH").unwrap_or_default();
    choose(&path, dev, link, |p| p.is_file())
}

#[cfg(not(target_os = "macos"))]
fn resolve() -> PathBuf {
    PathBuf::from("git")
}

/// Plain `git` unless the first `git` on `path` is exactly `/usr/bin/git` and the
/// developer dir (`$DEVELOPER_DIR`, else the xcode-select link, else the Command
/// Line Tools) holds a real one. Homebrew gits and test PATH shims stay as they are.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn choose(
    path: &std::ffi::OsStr,
    dev: Option<PathBuf>,
    link: Option<PathBuf>,
    is_file: impl Fn(&Path) -> bool,
) -> PathBuf {
    let plain = PathBuf::from("git");
    let first = std::env::split_paths(path)
        .map(|d| d.join("git"))
        .find(|p| is_file(p));
    if first.as_deref() != Some(Path::new("/usr/bin/git")) {
        return plain;
    }
    let dir = dev
        .or(link)
        .unwrap_or_else(|| PathBuf::from("/Library/Developer/CommandLineTools"));
    let real = dir.join("usr/bin/git");
    if is_file(&real) { real } else { plain }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_usr_bin_shim_is_bypassed() {
        let has = |p: &Path| {
            p == Path::new("/opt/homebrew/bin/git")
                || p.starts_with("/usr/bin")
                || p == Path::new("/Dev/usr/bin/git")
        };
        let dev = Some(PathBuf::from("/Dev"));
        let brew = std::ffi::OsString::from("/opt/homebrew/bin:/usr/bin");
        assert_eq!(choose(&brew, dev.clone(), None, has), PathBuf::from("git"));
        let sys = std::ffi::OsString::from("/usr/bin");
        assert_eq!(
            choose(&sys, dev, None, has),
            PathBuf::from("/Dev/usr/bin/git")
        );
        let gone = Some(PathBuf::from("/Nope"));
        assert_eq!(choose(&sys, gone, None, has), PathBuf::from("git"));
    }
}
