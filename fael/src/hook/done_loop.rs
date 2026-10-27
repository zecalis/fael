//! One line at `chunk done`: the repo's context-loop numbers, so the agent sees whether
//! hand-offs and closes are holding. Read from this repo's usage rows and log alone —
//! no git, no gh — and the same numbers `fael stats` prints, never a claim past them.

use super::usage::load_where;
use crate::{Repo, core};
use std::path::Path;

/// The window the line reads: 30 days of usage, as a month of `fael stats --since`.
const WINDOW_MS: u64 = 30 * 24 * 3600 * 1000;

/// `context loop (30d): confirmed repeats a of b edits after close`; nothing counted = no line.
pub(crate) fn line(r: &Repo) -> Option<String> {
    let want = crate::journal::root(&r.root).unwrap_or_else(|| r.root.clone());
    let since = core::now_ms().saturating_sub(WINDOW_MS) as i64;
    let u = load_where(Some(since), &|repo| {
        let root = crate::journal::root(Path::new(repo)).unwrap_or_else(|| repo.into());
        root == want
    });
    text(&core::stats::context_loop(&u.parsed, &u.logs))
}

fn text(c: &core::stats::ContextLoop) -> Option<String> {
    (c.confirmed_repeats + c.edits_after_close > 0).then(|| {
        format!(
            "context loop (30d): confirmed repeats {} of {} edits after close\n",
            c.confirmed_repeats, c.edits_after_close
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_when_nothing_counted() {
        assert_eq!(text(&Default::default()), None);
        let c = core::stats::ContextLoop {
            confirmed_repeats: 0,
            edits_after_close: 29,
            useful_shows: 0,
        };
        assert_eq!(
            text(&c).as_deref(),
            Some("context loop (30d): confirmed repeats 0 of 29 edits after close\n")
        );
    }
}
