//! Flake repeats: an `e2e:flake` (or `flake`-keyed) issue re-filed on the same
//! key and files counts on the same row instead of opening a new one. E2e
//! flakes recur with different messages on the same file, so text equality
//! would pile a row per run.

use super::evidence::Evidence;

/// A flake key: `e2e:flake` or carrying `flake` — the recurring-failure topic
/// whose repeats merge instead of piling rows.
pub(super) fn is_key(key: Option<&str>) -> bool {
    key.is_some_and(|k| {
        let k = k.to_lowercase();
        k.contains("flake") || k == "e2e:flake" || k.starts_with("e2e:flake:")
    })
}

/// Same files, exactly: every file of the new row is on the old one and vice
/// versa — a flake on `statement.ts` never swallows one on another file.
pub(super) fn same_files(ev: &Evidence) -> bool {
    ev.files.shared > 0 && ev.files.shared == ev.files.of_new && ev.files.shared == ev.files.of_old
}
