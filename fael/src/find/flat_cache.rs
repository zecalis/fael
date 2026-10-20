//! The flat files under `<git-common-dir>/fael/cache/` that hold facts which are a
//! pure function of git object ids (decision 01M5CRPS): lines of `<key> <value>`,
//! append-only, shared by every worktree. A torn or foreign line is skipped on
//! read, the last line for a key wins, and a file past `CACHE_MAX` is cut to its
//! newest half on load (lines are in the order they were learned). A cache, never
//! row state — losing a line costs one proof.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

// Every pull reads the whole file, and `merge-bases` gains lines each time HEAD moves.
const CACHE_MAX: u64 = 1 << 20;
/// The value for "git answered, and there is nothing": no patch, no merge base.
pub(super) const NONE: &str = "-";

pub(super) struct Cache {
    file: Option<PathBuf>,
    pub(super) map: HashMap<String, String>,
}

impl Cache {
    pub(super) fn load(root: &Path, name: &str) -> Self {
        let file = crate::journal::root(root).map(|j| j.join("cache").join(name));
        let mut map = HashMap::new();
        if let Some(f) = &file {
            let mut text = std::fs::read_to_string(f).unwrap_or_default();
            if text.len() as u64 > CACHE_MAX {
                text = newest_half(&text).to_string();
                // tmp + rename: a reader sees the old file or the cut one, never a torn one;
                // a line another worktree appends to the old inode meanwhile is lost (a cache)
                let tmp = f.with_extension(format!("cut{}", std::process::id()));
                if std::fs::write(&tmp, &text).is_err() || std::fs::rename(&tmp, f).is_err() {
                    let _ = std::fs::remove_file(&tmp);
                }
            }
            for l in text.lines() {
                if let Some((k, v)) = l.split_once(' ') {
                    map.insert(k.to_string(), v.to_string());
                }
            }
        }
        Cache { file, map }
    }

    /// Fails open: unwritable = the next process proves it again.
    pub(super) fn put(&mut self, entries: Vec<(String, String)>) {
        let body: String = entries.iter().map(|(k, v)| format!("{k} {v}\n")).collect();
        self.map.extend(entries);
        let Some(f) = &self.file else { return };
        let _ = f.parent().map(std::fs::create_dir_all);
        // one write on an O_APPEND handle: two worktrees never interleave a line
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(f)
            .and_then(|mut h| h.write_all(body.as_bytes()));
    }
}

/// The last `CACHE_MAX / 2` bytes, from the first whole line.
fn newest_half(text: &str) -> &str {
    let mut from = text.len() - (CACHE_MAX as usize / 2);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    match text[from..].find('\n') {
        Some(i) => &text[from + i + 1..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_big_file_keeps_its_newest_lines_and_drops_the_oldest() {
        let body: String = (0..30_000).map(|i| format!("k{i:06} {i:040}\n")).collect();
        assert!(body.len() as u64 > CACHE_MAX);
        let cut = newest_half(&body);
        assert!(cut.len() as u64 <= CACHE_MAX / 2);
        assert!(cut.starts_with('k'), "starts on a whole line");
        assert!(cut.ends_with(&format!("k029999 {:040}\n", 29_999)));
        assert!(!cut.contains("k000000 "));
    }
}
