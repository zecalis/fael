//! Per-session scratch in the state dir (decision 01M5WC02, class C): every
//! file here is a per-session note, a per-clone cache or a last-run log, so
//! losing an old one only means a row is said again or a cache rebuilt. The
//! ledger and `usage.jsonl` are not touched.

use super::state::state_dir;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Scratch untouched this long is dead. Every writer rewrites its file on use,
/// so mtime is last use.
const TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// At most one pass per this long: a scan of ~30k files is not a start-up cost.
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Dirs of per-session or per-clone files, flat.
const DIRS: [&str; 7] = [
    "sessions",
    "sessions/edits",
    "held",
    "held-shown",
    "retired",
    "spent",
    "sync-seen",
];

fn age(p: &Path) -> Option<Duration> {
    SystemTime::now()
        .duration_since(p.metadata().ok()?.modified().ok()?)
        .ok()
}

fn prune_dir(dir: &Path, keep: impl Fn(&str) -> bool) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        if keep(&name.to_string_lossy()) && age(&e.path()).is_some_and(|a| a > TTL) {
            // a sub-directory (`sessions/edits`) fails here and is walked on its own
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Session-start: prune once a day. Best-effort — every error is ignored and
/// the next day tries again.
pub(crate) fn prune_state() {
    let dir = state_dir();
    let stamp = dir.join("prune.stamp");
    if age(&stamp).is_some_and(|a| a < EVERY) {
        return;
    }
    // stamp first: a start racing this one skips instead of scanning too
    if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&stamp, "").is_err() {
        return;
    }
    for d in DIRS {
        prune_dir(&dir.join(d), |_| true);
    }
    // per-worktree last-run logs and the rebuildable git-log cache
    prune_dir(&dir, |n| {
        n.starts_with("auto-sync-") || n.starts_with("drift-")
    });
    // class B2: raw usage past 3 months, after its monthly summary is written
    super::usage_retain::retain();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aged(p: &Path, by: Duration) {
        std::fs::write(p, "x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(SystemTime::now() - by)
            .unwrap();
    }

    #[test]
    fn prunes_stale_scratch_once_a_day() {
        let d = std::env::temp_dir().join(format!("fael-prune-{}", fael_core::ulid()));
        let old = TTL + Duration::from_secs(60);
        let files = [
            "sessions/a.seen",
            "spent/a.tsv",
            "auto-sync-1.log",
            "drift-1.txt",
        ];
        for f in files {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            aged(&d.join(f), old);
        }
        aged(&d.join("sessions/new.seen"), Duration::ZERO);
        aged(&d.join("usage.jsonl"), old); // not scratch: never touched
        // SAFETY: this crate's tests set FAEL_STATE_DIR only through this one
        // test, and `cargo nextest` runs each test in its own process.
        unsafe { std::env::set_var("FAEL_STATE_DIR", &d) };
        prune_state();
        assert!(files.iter().all(|f| !d.join(f).exists()));
        assert!(d.join("sessions/new.seen").exists() && d.join("usage.jsonl").exists());
        // the stamp skips a second pass the same day
        aged(&d.join("sessions/b.seen"), old);
        prune_state();
        assert!(d.join("sessions/b.seen").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
