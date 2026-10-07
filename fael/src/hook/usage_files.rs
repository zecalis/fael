//! Where usage lives so a read stays bounded: `usage.jsonl` is the live file,
//! and at the first write of a new month it moves to `usage/<YYYY-MM>.jsonl`,
//! named by the month of its last write. An archive therefore holds rows up to
//! its month and after the previous archive's — a read from a month on opens
//! only the archives named from it. A read with no `since` opens the newest
//! archive and the live file: this month and the last, never the whole history.

use super::state::state_dir;
use crate::core;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(crate) fn live() -> PathBuf {
    state_dir().join("usage.jsonl")
}

fn archive_dir() -> PathBuf {
    state_dir().join("usage")
}

fn month(ms: u64) -> String {
    core::rfc3339(ms)[..7].to_string()
}

fn mtime_ms(p: &Path) -> Option<u64> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// Before an append: archive a live file last written in an earlier month.
/// `hard_link` refuses an archive that exists, so of racing writers exactly one
/// moves it; a writer still holding the old fd lands its row in the archive.
// ponytail: a crash between link and unlink leaves live and archive one inode
// and rotation stuck for that month; same-inode check + unlink if it happens.
pub(crate) fn rotate() {
    let live = live();
    let Some(last) = mtime_ms(&live).map(month) else {
        return;
    };
    if last >= month(core::now_ms()) {
        return;
    }
    let dir = archive_dir();
    if std::fs::create_dir_all(&dir).is_ok()
        && std::fs::hard_link(&live, dir.join(format!("{last}.jsonl"))).is_ok()
    {
        let _ = std::fs::remove_file(&live);
    }
}

/// The month of the live file's last write (now when there is none). An
/// archive named from it on can only come from moving that live file later.
pub(crate) fn live_month() -> String {
    month(mtime_ms(&live()).unwrap_or_else(core::now_ms))
}

/// The archives named `from` on, oldest first.
pub(crate) fn archives_from(from: &str) -> Vec<PathBuf> {
    let mut a = all_archives();
    a.retain(|p| {
        p.file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s >= from)
    });
    a
}

/// The archives a read covers, oldest first: from `since`'s month on, or the
/// newest one alone when there is no `since`.
fn archives(since: Option<i64>) -> Vec<PathBuf> {
    if let Some(ms) = since {
        return archives_from(&month(ms.max(0) as u64));
    }
    let mut a = all_archives();
    let old = a.len().saturating_sub(1);
    a.drain(..old);
    a
}

fn all_archives() -> Vec<PathBuf> {
    let mut a: Vec<PathBuf> = std::fs::read_dir(archive_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    a.sort();
    a
}

/// The usage past byte `from` of the live file as it was in `month`: archives
/// named from `month` on are that file moved since — the oldest is read from
/// `from`, any later one whole, then the new live file from its start. Gives the
/// text, where the live file ends now, and its month (read before the text: a
/// move after it names its archive from this month on).
pub(crate) fn read_since(month: &str, from: u64) -> (String, u64, String) {
    let live = live();
    let now = live_month();
    let mut text = Vec::new();
    let moved = archives_from(month);
    for (i, a) in moved.iter().enumerate() {
        tail(a, if i == 0 { from } else { 0 }, &mut text);
    }
    let at = if moved.is_empty() { from } else { 0 };
    // a live file shorter than `at` was cut by hand: count it from its start
    let end = tail(&live, at, &mut text)
        .or_else(|| tail(&live, 0, &mut text))
        .unwrap_or(0);
    (String::from_utf8_lossy(&text).into_owned(), end, now)
}

/// Append `file`'s bytes past `from` to `into`; its length, or `None` when it
/// is missing or shorter than `from`.
fn tail(file: &Path, from: u64, into: &mut Vec<u8>) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(file).ok()?;
    let len = f.metadata().ok()?.len();
    if len < from {
        return None;
    }
    f.seek(SeekFrom::Start(from)).ok()?;
    f.read_to_end(into).ok()?;
    Some(len)
}

/// The usage text a read covers: its archives, then the live file.
pub(crate) fn read(since: Option<i64>) -> String {
    archives(since)
        .into_iter()
        .chain([live()])
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::month;

    #[test]
    fn month_is_the_utc_year_month() {
        assert_eq!(month(0), "1970-01");
        assert_eq!(month(1_790_000_000_000), "2026-09");
    }
}
