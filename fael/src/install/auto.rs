//! Self-update (PLAN-fael-auto-update chunk 3). Session start, at most once a
//! day, spawns `fael upgrade --auto` detached — never awaited, so the hook costs
//! nothing and a dead network costs nothing. The child looks for a newer release
//! tag (`git ls-remote`, git is already required), waits until it has seen that
//! tag for a day so a bad release meets its owner first, then runs the channel's
//! update and the new binary's wiring pass. What it did lands in
//! `<state>/update.json`; the next session start says one Notice and takes the
//! result out. Off: `FAEL_NO_AUTO_UPDATE=1` or `auto_update = false` in
//! `~/.config/fael/config.toml` — while off no check starts and a stored
//! receipt is taken silently; a hand-run `fael upgrade` is never affected.
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
    #[serde(skip_serializing_if = "Option::is_none")]
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

fn save(s: &State) -> bool {
    let Ok(j) = serde_json::to_string(s) else {
        return false;
    };
    std::fs::create_dir_all(state_dir())
        .and_then(|()| std::fs::write(path(), j))
        .is_ok()
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
    if !save(&s) {
        // nowhere to stamp the check — spawning now would respawn every session
        return;
    }
    let _ = Command::new(exe)
        .args(["upgrade", "--auto"])
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn();
}

/// The take in notice() happens under this guard: `create_new` is atomic, so
/// of two sessions starting at once only one takes the receipt. A caller that
/// finds a live holder defers (the holder says the line); a lock older than a
/// minute is a crashed session's and gets stolen, so no crash hushes every
/// session after it. Removed when the section ends.
struct TakeGuard(PathBuf);

impl Drop for TakeGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn take_guard(dir: &Path) -> Option<TakeGuard> {
    let p = dir.join("update.lock");
    for _ in 0..20 {
        match OpenOptions::new().create_new(true).write(true).open(&p) {
            Ok(_) => return Some(TakeGuard(p)),
            Err(_) => {
                let stale = std::fs::metadata(&p)
                    .and_then(|m| m.modified())
                    .map(|t| t.elapsed().map(|d| d.as_secs() > 60).unwrap_or(true))
                    .unwrap_or(true);
                if stale {
                    let _ = std::fs::remove_file(&p);
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }
    None
}

/// Session start, next session: the one line for the last check's result, once.
/// Opted out, the receipt is still taken but stays silent — re-enabling says
/// nothing stale.
pub(crate) fn notice() -> Option<String> {
    // a contended lock means another session is taking it right now — defer,
    // it says the line; only a lone holder takes
    let _guard = take_guard(&state_dir())?;
    let mut s = load();
    let result = s.result.take()?;
    save(&s);
    if off() {
        return None;
    }
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
/// Flags that scope a hand-run upgrade are rejected, never silently ignored
/// (--dry-run would be the worst: a real update wearing dry-run).
pub(crate) fn run(dry: bool, client: bool, replace: bool) -> Result<(), String> {
    if dry || client || replace {
        return Err(
            "rejected: `upgrade --auto` takes no --dry-run, --client or --replace-fapony".into(),
        );
    }
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

    /// Scoped env override (no other unit test touches these keys).
    struct Env(&'static str, Option<std::ffi::OsString>);

    impl Env {
        fn set(key: &'static str, val: &str) -> Self {
            let old = std::env::var_os(key);
            unsafe {
                std::env::set_var(key, val);
            }
            Self(key, old)
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var_os(key);
            unsafe {
                std::env::remove_var(key);
            }
            Self(key, old)
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            unsafe {
                match &self.1 {
                    Some(v) => std::env::set_var(self.0, v),
                    None => std::env::remove_var(self.0),
                }
            }
        }
    }

    /// Of two sessions starting at once only one holds the take guard; once
    /// freed it retakes.
    #[test]
    fn the_take_guard_is_exclusive() {
        let dir = std::env::temp_dir().join(format!("fael-guard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let held = super::take_guard(&dir).expect("first take");
        let rival = std::thread::spawn({
            let dir = dir.clone();
            move || super::take_guard(&dir)
        })
        .join()
        .unwrap();
        assert!(rival.is_none(), "two holders of one receipt");
        drop(held);
        assert!(super::take_guard(&dir).is_some(), "freed guard retakes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A crashed session's lock is stolen, never hushes every session after it.
    #[test]
    fn a_stale_take_guard_is_stolen() {
        let dir = std::env::temp_dir().join(format!("fael-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = std::fs::File::create(dir.join("update.lock")).unwrap();
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(61);
        f.set_modified(past).unwrap();
        drop(f);
        assert!(super::take_guard(&dir).is_some(), "stale lock not stolen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Eight sessions starting on one stored receipt: exactly one says it.
    #[test]
    fn concurrent_notices_say_a_receipt_once() {
        use std::sync::Barrier;
        let base = std::env::temp_dir().join(format!("fael-take-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let state = base.join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(
            state.join("update.json"),
            r#"{"checked_at":1,"seen_at":0,"from":"0.30.0","to":"99.0.0","result":"updated"}"#,
        )
        .unwrap();
        let _state_dir = Env::set("FAEL_STATE_DIR", state.to_str().unwrap());
        let _cfg = Env::set("XDG_CONFIG_HOME", base.join("cfg").to_str().unwrap());
        let _opt_out = Env::remove("FAEL_NO_AUTO_UPDATE");
        let barrier = Barrier::new(8);
        std::thread::scope(|s| {
            let mut handles = vec![];
            for _ in 0..8 {
                handles.push(s.spawn(|| {
                    barrier.wait();
                    super::notice()
                }));
            }
            let said: Vec<_> = handles
                .into_iter()
                .filter_map(|h| h.join().unwrap())
                .collect();
            assert_eq!(said.len(), 1, "{said:?}");
            assert!(said[0].contains("updated 0.30.0 → 99.0.0"), "{}", said[0]);
        });
        let json = std::fs::read_to_string(state.join("update.json")).unwrap_or_default();
        assert!(!json.contains("\"result\""), "{json}");
        assert!(!state.join("update.lock").exists(), "guard left behind");
        let _ = std::fs::remove_dir_all(&base);
    }
}
