//! Which notes `[Shipped]` may close on its own. A landed branch ends a
//! note only when the note says it is a status of that work (a PR opened,
//! a chunk done, what shipped, not yet committed). A standing rule or a fact
//! filed from the same branch stays true after the merge, so it is listed
//! (`[ShippedKept]`) and never `--fix` closed — the rule is what the note
//! says, never a guess at what it means (issue 01M4164E).
//!
//! Judged on the note's opening only (its title, and the first line of its
//! text) against a closed list of whole-word phrases: the later lines of a
//! rule often retell the incident that taught it ("as happened when a commit
//! landed on …"), which says nothing about the note being a status.

use crate::core;

/// Word sequences that mark a status: each is matched as consecutive
/// lowercase words, so `pr opened` does not match `improved`.
const PHRASES: &[&[&str]] = &[
    &["pr", "opened"],
    &["opened", "pr"],
    &["pr", "open"],
    &["pr", "merged"],
    &["pr", "rebased"],
    &["no", "pr", "yet"],
    &["rebased", "on"],
    &["what", "shipped"],
    &["shipped", "in"],
    &["not", "yet", "committed"],
    &["ready", "for", "review"],
];

/// A status line names its chunk: `chunk 5 done`, `chunk 2 shipped`.
const CHUNK_ENDS: &[&str] = &["done", "shipped", "landed"];

/// True when the note's title or the first line of its text reads as a
/// status of the work on its branch. `half done`, `not done` and any rule or
/// fact stay false.
pub(super) fn is_status(row: &core::Row) -> bool {
    let first = row.text.lines().next().unwrap_or("");
    row.title
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(first))
        .any(opening_is_status)
}

fn opening_is_status(s: &str) -> bool {
    let lower = s.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    if PHRASES
        .iter()
        .any(|p| words.windows(p.len()).any(|w| w == *p))
    {
        return true;
    }
    words.windows(3).any(|w| {
        w[0] == "chunk"
            && !w[1].is_empty()
            && w[1].chars().all(|c| c.is_ascii_digit())
            && CHUNK_ENDS.contains(&w[2])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: Option<&str>, text: &str) -> core::Row {
        core::Row {
            title: title.map(str::to_string),
            text: text.into(),
            ..core::Row::new("t", "note", text, vec![])
        }
    }

    #[test]
    fn status_shapes_are_closable() {
        for text in [
            "say-gate chunk 5 done (0ccc826, branch feat/x, no PR yet): stats text",
            "PR opened for the doctor fix",
            "Rebased on main after #193; nothing else changed",
            "What shipped: the outbox",
            "Not yet committed: the stats change sits in the worktree",
            "chunk 2 shipped, next is chunk 3",
        ] {
            assert!(is_status(&note(None, text)), "{text}");
        }
        assert!(is_status(&note(Some("PR opened for #9"), "see body")));
    }

    #[test]
    fn rules_and_facts_are_kept() {
        // the shapes named in 01M4164E: standing rules and facts filed from a
        // branch that later landed
        for text in [
            "On 2026-10-03, 523 benchmark usage rows were removed from the real usage.jsonl. Rule for the next benchmark: point FAEL_STATE_DIR at a scratch dir.",
            "Worktree wt-x exists for push-focus work. Do NOT set CARGO_TARGET_DIR to a shared dir: parallel agents then overwrite each other's target/debug/fael.",
            "Verify branch with git branch --show-current before git push, as happened when a commit landed on fix/a instead of feat/b",
            "OpenCode 1.18.34 exports no session env var to shell or MCP (read from the bundled code, not yet run).",
            "01M3XFRKW is half done on fix/push-hint-precision: the rule was left out",
            "chunk 5 not done: the stats text is missing",
        ] {
            assert!(!is_status(&note(None, text)), "{text}");
        }
    }

    #[test]
    fn only_the_opening_counts() {
        // a status phrase on a later line of a rule does not make it a status
        let row = note(
            Some("Benchmarks must isolate state"),
            "Point FAEL_STATE_DIR at a scratch dir.\nLast time the PR opened with polluted stats.",
        );
        assert!(!is_status(&row));
    }
}
