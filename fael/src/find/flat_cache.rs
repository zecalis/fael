//! The flat files under `<git-common-dir>/fael/cache/` that hold facts which are a
//! pure function of git object ids (decision 01M5CRPS): lines of `<key> <value>`,
//! append-only, shared by every worktree. A torn or foreign line is skipped on
//! read, the last line for a key wins, and a file past `CACHE_MAX` is dropped whole
//! and rebuilt. A cache, never row state — losing it costs one proof.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

// ponytail: dropped whole and rebuilt; a compaction pass if a repo ever outgrows
// it between rebuilds
const CACHE_MAX: u64 = 8 << 20;
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
            if std::fs::metadata(f).is_ok_and(|m| m.len() > CACHE_MAX) {
                let _ = std::fs::remove_file(f);
            }
            for l in std::fs::read_to_string(f).unwrap_or_default().lines() {
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
