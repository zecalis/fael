//! The `add` arity reject, said with the ready retype line. Never picks the
//! kind (§0): with none given, the line carries the `<kind>` slot; with a kind
//! and loose words, they are only quoted — the agent's own words, unchanged.

use crate::core::CORE_KINDS;

/// `e` (the generic "wrong arguments" reject for `add`) with the retype line
/// appended; `e` itself when the argv is not one of the three known shapes.
pub(crate) fn explain(argv: &[String], e: String) -> String {
    if argv.first().map(String::as_str) != Some("add")
        || !e.starts_with("rejected: wrong arguments")
    {
        return e;
    }
    let Ok(a) = crate::Args::parse(argv.to_vec()) else {
        return e;
    };
    let rest = a.pos.get(1..).unwrap_or_default();
    let (kind, words) = match rest.split_first() {
        Some((k, w)) if CORE_KINDS.contains(&k.as_str()) => (k.as_str(), w),
        Some(_) => ("<kind>", rest),
        // bare `fael add`: no kind, no text — the retype line with the
        // kind slot open, so the agent retypes without a `--help` round.
        None => {
            let files = a
                .one("files")
                .map_or(" --files <files>".to_string(), |f| format!(" --files {f}"));
            let title = a
                .one("title")
                .map_or("<≤15-word headline>".to_string(), String::from);
            return format!(
                "rejected: wrong arguments for \"add\" — add needs a kind first (decision, issue or note); retype: fael add decision|issue|note \"<text>\" --title \"{title}\"{files}"
            );
        }
    };
    let text = words.join(" ");
    let (why, text) = match (kind, words.len()) {
        ("<kind>", _) => (
            "add needs a kind first (decision, issue or note)",
            text.replace('"', "\\\""),
        ),
        (_, 0) => ("add needs the text after the kind", "<text>".into()),
        _ => (
            "add takes the text as one quoted argument",
            text.replace('"', "\\\""),
        ),
    };
    let files = a
        .one("files")
        .map_or(String::new(), |f| format!(" --files {f}"));
    let title = a
        .one("title")
        .map_or("<≤15-word headline>".to_string(), String::from);
    format!(
        "rejected: wrong arguments for \"add\" — {why}; retype: fael add {kind} \"{text}\" --title \"{title}\"{files}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &str) -> String {
        let argv: Vec<String> = s.split(' ').map(String::from).collect();
        explain(
            &argv,
            "rejected: wrong arguments for \"add\" — try 'fael add --help'".into(),
        )
    }

    #[test]
    fn no_kind_keeps_the_slot() {
        let got = run("add some_text");
        assert!(
            got.contains("fael add <kind> \"some_text\" --title"),
            "{got}"
        );
        assert!(got.contains("needs a kind"), "{got}");
        // keeps the needle the friction code `usage` reads
        assert!(got.starts_with("rejected: wrong arguments"), "{got}");
    }

    #[test]
    fn bare_add_prints_retype_line() {
        let got = run("add");
        assert!(
            got.contains("fael add decision|issue|note \"<text>\" --title"),
            "{got}"
        );
        assert!(got.contains("--files <files>"), "{got}");
        assert!(got.contains("needs a kind"), "{got}");
        // keeps the needle the friction code `usage` reads
        assert!(got.starts_with("rejected: wrong arguments"), "{got}");
    }

    #[test]
    fn bare_add_keeps_given_files() {
        let got = run("add --files x.rs");
        assert!(got.ends_with("--files x.rs"), "{got}");
    }

    #[test]
    fn unquoted_words_are_quoted() {
        let got = run("add note a b c");
        assert!(got.contains("fael add note \"a b c\" --title"), "{got}");
    }

    #[test]
    fn kept_files_and_other_errors_pass() {
        let got = run("add --files x.rs some_text");
        assert!(got.ends_with("--files x.rs"), "{got}");
        let other = "rejected: unknown flag --x".to_string();
        assert_eq!(explain(&["add".into()], other.clone()), other);
    }
}
