//! Brief paths the planner names without asking for an edit (issue 01M5D5FCS):
//! "do not touch X" and "run X". Mechanical, never a judgement: a fixed phrase in the
//! same sentence before the path, or the word "run" right before it.

/// Phrases that put the paths after them out of the edit set (lowercase).
const NEGATIONS: &[&str] = &[
    "do not touch",
    "don't touch",
    "do not edit",
    "don't edit",
    "leave",
    "never",
    "not touch",
];

/// Words that make the next path a command to run, not a file to edit.
const RUN_WORDS: &[&str] = &["run", "runs", "running"];

/// Words that make the next path a doc to read, not a file to edit (issue 01M5H3FG8).
const READ_WORDS: &[&str] = &["read", "reads", "see", "per", "follow"];

/// Sentence pieces of `text`: split at `.` `;` and newline only where a space or the end
/// follows a `.`, so `foo.rs` and `a/b.sh` stay whole.
pub(crate) fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let end = match c {
            '\n' | ';' => true,
            '.' => chars.peek().is_none_or(|&(_, n)| n.is_whitespace()),
            _ => false,
        };
        if end {
            out.push(&text[start..i]);
            start = i + c.len_utf8();
        }
    }
    out.push(&text[start..]);
    out
}

/// True when `path` in `sentence` follows a negation phrase, or the word "run" / "read".
pub(crate) fn skipped(sentence: &str, path: &str) -> bool {
    let Some(at) = sentence.find(path) else {
        return false;
    };
    let before = sentence[..at].to_lowercase();
    if NEGATIONS.iter().any(|n| before.contains(n)) {
        return true;
    }
    let prev = before
        .trim_end_matches(|c: char| c == '`' || c.is_whitespace())
        .rsplit(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or("");
    RUN_WORDS.contains(&prev) || READ_WORDS.contains(&prev)
}

#[cfg(test)]
mod tests {
    use super::{sentences, skipped};

    #[test]
    fn negated_and_run_paths_are_skipped() {
        let s = "Do not touch `fael/src/maintain/noverdict.rs` here";
        assert!(skipped(s, "fael/src/maintain/noverdict.rs"));
        assert!(skipped("Then run `scripts/gate.sh`", "scripts/gate.sh"));
        assert!(skipped("Running scripts/gate.sh", "scripts/gate.sh"));
    }

    #[test]
    fn read_words_skip_the_path_but_an_edit_sentence_still_warns() {
        assert!(skipped("First read docs/ACCESS.md", "docs/ACCESS.md"));
        assert!(skipped("Shape per `docs/PAGES.md`", "docs/PAGES.md"));
        assert!(skipped("See docs/A.md", "docs/A.md"));
        assert!(!skipped("Edit docs/ACCESS.md to add it", "docs/ACCESS.md"));
        assert!(!skipped("Read it, then edit docs/A.md", "docs/A.md"));
    }

    #[test]
    fn a_plain_mention_still_warns() {
        assert!(!skipped("Edit fael/src/x.rs to add it", "fael/src/x.rs"));
        assert!(!skipped("Then gate scripts/gate.sh", "scripts/gate.sh"));
    }

    #[test]
    fn a_negation_in_another_sentence_does_not_suppress() {
        let text = "Do not touch other.rs. Edit fael/src/x.rs now";
        let parts = sentences(text);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(skipped(parts[0], "other.rs"));
        assert!(!skipped(parts[1], "fael/src/x.rs"));
        let nl = sentences("never edit a.rs\nEdit b/c.rs");
        assert!(!skipped(nl[1], "b/c.rs"));
    }
}
