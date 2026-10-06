//! Self-update (PLAN-fael-auto-update chunk 3). Session start, at most once a
//! day, spawns `fael upgrade --auto` detached — never awaited, so the hook costs
//! nothing and a dead network costs nothing. The child looks for a newer release
//! tag (`git ls-remote`, git is already required), waits until it has seen that
//! tag for a day so a bad release meets its owner first, then runs the channel's
//! update and the new binary's wiring pass. What it did lands in
//! `<state>/update.json`; the next session start says one Notice and takes the
//! result out. Off: `FAEL_NO_AUTO_UPDATE=1` or `auto_update = false` in
//! `~/.config/fael/config.toml`; a hand-run `fael upgrade` is never affected.
// ponytail: the tag's age is "seen for a day" (ls-remote has no dates), so a
// release lands one to two days after it is out; the log is appended, never
// truncated (~1 KB a day); a failed update retries daily and says so each time.

use super::upgrade;
use crate::core::stats::state_dir;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DAY: u64 = 86_400;
const REMOTE: &str = "https://github.com/zecalis/fael";

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct State {
    /// unix seconds; the spawner stamps it before it starts the child
    checked_at: u64,
    from: String,
    /// the newest tag seen, without its `v`
    to: String,
    /// `checked_at` of the check that first saw `to`
    seen_at: u64,
    /// `updated` | `available` | `failed` — said once, then taken
    result: Option<String>,
    why: String,
    log: String,
}

fn path() -> PathBuf {
    state_dir().join("update.json")
}

fn load() -> State {
    let s = std::fs::read_to_string(path()).unwrap_or_default();
    serde_json::from_str(&s).unwrap_or_default()
}

fn save(s: &State) {
    let _ = std::fs::create_dir_all(state_dir());
    let _ = serde_json::to_string(s).map(|j| std::fs::write(path(), j));
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn off() -> bool {
    if std::env::var("FAEL_NO_AUTO_UPDATE").is_ok_and(|v| !v.is_empty() && v != "0") {
        return true;
    }
    let cfg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::home().map(|h| h.join(".config")));
    cfg.and_then(|d| std::fs::read_to_string(d.join("fael/config.toml")).ok())
        .is_some_and(|s| !crate::core::auto_update_on(&s))
}

/// Session start: when the last check is a day old, stamp it and start the child.
pub(crate) fn start() {
    let mut s = load();
    if now().saturating_sub(s.checked_at) < DAY || off() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let log = state_dir().join("update.log");
    let f = std::fs::create_dir_all(state_dir())
        .ok()
        .and_then(|()| OpenOptions::new().create(true).append(true).open(&log).ok());
    let Some((out, err)) = f.and_then(|f| f.try_clone().ok().map(|c| (f, c))) else {
        return;
    };
    s.checked_at = now();
    s.log = log.display().to_string();
    save(&s);
    let _ = Command::new(exe)
        .args(["upgrade", "--auto"])
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn();
}

/// Session start, next session: the one line for the last check's result, once.
pub(crate) fn notice() -> Option<String> {
    let mut s = load();
    let result = s.result.take()?;
    save(&s);
    let (from, to) = (&s.from, &s.to);
    Some(match result.as_str() {
        "updated" => format!("fael: updated {from} → {to} · wiring current\n"),
        "available" => format!("fael: {to} is out — run `fael upgrade`\n"),
        _ => format!(
            "fael: auto-update to {to} failed ({}) — run fael upgrade, log: {}\n",
            s.why.lines().next().unwrap_or(""),
            s.log
        ),
    })
}

/// `fael upgrade --auto`: the detached child. Silent unless there is something to say.
pub(crate) fn run() -> Result<(), String> {
    if off() {
        return Ok(());
    }
    let home = crate::home().ok_or("no home directory")?;
    let exe = std::env::current_exe()
        .and_then(|e| e.canonicalize())
        .map_err(|e| e.to_string())?;
    // nothing to run (a dev build, `cargo install`): no network either
    let ch = upgrade::detect(&exe, &upgrade::cargo_home(&home));
    if upgrade::command(&ch, &exe).is_none() {
        return Ok(());
    }
    let from = env!("CARGO_PKG_VERSION");
    let remote = std::env::var("FAEL_UPDATE_REMOTE").unwrap_or_else(|_| REMOTE.into());
    let Some(to) = newest(&ls_remote(&remote), from) else {
        return Ok(()); // offline, or already the newest
    };
    let mut s = load();
    s.from = from.into();
    if s.to != to {
        println!("{to} is out — installs after a day");
        s.to = to;
        s.seen_at = s.checked_at;
        // a running exe cannot be replaced on Windows (yet): tell, never install
        s.result = cfg!(windows).then(|| "available".into());
        save(&s);
        return Ok(());
    }
    if cfg!(windows) || s.checked_at.saturating_sub(s.seen_at) < DAY {
        return Ok(());
    }
    match update(&home, &exe, &to) {
        Ok(()) => (s.result, s.why) = (Some("updated".into()), String::new()),
        Err(e) => {
            println!("{e}");
            (s.result, s.why) = (Some("failed".into()), e);
        }
    }
    save(&s);
    Ok(())
}

/// The channel's update and the new binary's wiring, then proof the new binary
/// is the release: a tap or registry that lags leaves the old one in place.
fn update(home: &Path, exe: &Path, to: &str) -> Result<(), String> {
    if !upgrade::binary(home, &[], false, true)? {
        return Err("no update command ran".into());
    }
    let new = super::hook_exe().map_or(exe.to_path_buf(), PathBuf::from);
    let out = Command::new(new)
        .arg("--version")
        .output()
        .map_err(|e| format!("the new binary did not start: {e}"))?;
    let v = String::from_utf8_lossy(&out.stdout);
    match v.trim().strip_prefix("fael ") {
        Some(v) if v == to => Ok(()),
        v => Err(format!(
            "the binary is still {} after the update — {to} is not published to this channel yet",
            v.unwrap_or("unknown")
        )),
    }
}

fn ls_remote(remote: &str) -> String {
    Command::new("git")
        .args(["ls-remote", "--tags", "--refs", remote])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn ver(s: &str) -> Option<(u64, u64, u64)> {
    let mut p = s.strip_prefix('v').unwrap_or(s).split('.');
    let v = (
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
    );
    p.next().is_none().then_some(v)
}

/// The highest `vX.Y.Z` tag of `git ls-remote --tags --refs` output that is
/// newer than `current`, as `X.Y.Z`. A pre-release or any other tag is skipped.
fn newest(ls: &str, current: &str) -> Option<String> {
    let cur = ver(current)?;
    ls.lines()
        .filter_map(|l| ver(l.split_once("refs/tags/")?.1.trim()))
        .filter(|v| *v > cur)
        .max()
        .map(|(a, b, c)| format!("{a}.{b}.{c}"))
}

#[cfg(test)]
mod tests {
    use super::newest;

    #[test]
    fn the_newest_release_tag_beats_the_running_version() {
        let ls = "aa\trefs/tags/v0.9.0\nbb\trefs/tags/v0.30.0\ncc\trefs/tags/v0.31.0-rc.1\n\
                  dd\trefs/tags/nightly\nee\trefs/tags/v0.29.1\n";
        // 0.30 > 0.9 and 0.29.1: numeric, not text, order; the rc and the odd tag are skipped
        assert_eq!(newest(ls, "0.29.0").as_deref(), Some("0.30.0"));
        assert_eq!(newest(ls, "0.30.0"), None, "already the newest");
        assert_eq!(newest("", "0.29.0"), None, "offline reads as nothing");
    }
}
