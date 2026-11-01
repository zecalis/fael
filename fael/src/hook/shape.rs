//! Call shape for reject/help rows (PLAN-fael-friction-2 shape-log): the form
//! of a call that did not land — its verb, flag names and positional count —
//! never a value, so the next diagnosis reads no transcript. Pure: argv in,
//! shape out; `friction` writes it onto the call row on reject/help only.

/// A call's form: `verb` is the sub-command when it names a real `chunk`/`plan`
/// verb (closed vocabulary, like `sub`); `flags` the flag names used (sorted,
/// deduped, no values); `npos` the positional count past the command and verb.
pub(crate) struct Shape {
    pub verb: Option<String>,
    pub flags: Vec<String>,
    pub npos: usize,
}

/// Flags that take a value (`args.rs`): the word after one is the value, never
/// a positional. Keep beside `Args::parse`'s list — a new value flag there
/// miscounts `npos` here until it is added too.
const VALUE_FLAGS: &[&str] = &[
    "after",
    "app",
    "area",
    "before",
    "brief",
    "by",
    "check",
    "client",
    "cost",
    "drop",
    "edge",
    "files",
    "follows",
    "from",
    "issue",
    "key",
    "kind",
    "label",
    "left",
    "limit",
    "map",
    "model",
    "offset",
    "on",
    "option",
    "out",
    "parent",
    "plan",
    "pr",
    "reads",
    "recall",
    "replace",
    "returns",
    "review",
    "run",
    "scope",
    "section",
    "since",
    "size",
    "supersedes",
    "text",
    "then",
    "title",
    "to",
    "uid",
    "until",
    "urgent-before",
    "verdict",
    "why",
    "with",
    "worktree",
    "writer",
];

/// `--name` or `--name=value` (else `None`), plus whether the value rides
/// inline. A word with a space and no `=` (`"--secret words"`) is a value
/// wearing dashes — a positional, never a flag; the value is never stored.
fn long_flag(w: &str) -> Option<(&str, bool)> {
    let rest = w.strip_prefix("--")?;
    if rest.is_empty() || rest.starts_with('-') {
        return None;
    }
    match rest.split_once('=') {
        Some((n, _)) => Some((n, true)),
        None if rest.contains(char::is_whitespace) => None,
        None => Some((rest, false)),
    }
}

/// `-k`: a dash and a word — never `-` (stdin), `-1`, or text with spaces.
/// Mirrors `Args`' short-flag rule, so the same words read as flags here.
fn short_flag(w: &str) -> Option<&str> {
    let mut c = w.chars();
    (c.next() == Some('-')
        && c.next().is_some_and(|c| c.is_ascii_alphabetic())
        && !w.contains(char::is_whitespace))
    .then(|| &w[1..])
}

/// A flaggable name — letters, digits, dashes. Anything else (a value wearing
/// dashes) is dropped: never stored, never counted.
fn flag_name(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub(crate) fn shape(cmd: &str, argv: &[String]) -> Shape {
    // the verb seat: the word after `chunk`/`plan`, real verb or not — a flag
    // there is still a flag, but a word there is never a positional
    let cmd_at = argv
        .iter()
        .position(|w| !w.starts_with('-') && w != "help" && crate::help::for_command(w).is_some());
    let seat = cmd_at
        .filter(|_| cmd == "chunk" || cmd == "plan")
        .map(|c| c + 1);
    let mut flags: Vec<String> = vec![];
    let mut npos = 0;
    let mut i = 0;
    let mut dash = false;
    while i < argv.len() {
        let w = argv[i].as_str();
        i += 1;
        if !dash && w == "--" {
            dash = true;
            continue;
        }
        if !dash && let Some((n, inline)) = long_flag(w) {
            if flag_name(n) {
                push(&mut flags, n);
                if !inline && VALUE_FLAGS.contains(&n) {
                    if i < argv.len() {
                        i += 1;
                    }
                } else if !inline
                    && n == "revisit"
                    && argv.get(i).is_some_and(|v| !v.starts_with("--"))
                {
                    i += 1;
                }
            }
            continue;
        }
        if !dash && let Some(n) = short_flag(w) {
            push(&mut flags, n);
            continue;
        }
        let at = i - 1;
        if Some(at) == cmd_at || seat == Some(at) || (at == 0 && argv[0] == "help") {
            continue;
        }
        npos += 1;
    }
    flags.sort();
    flags.dedup();
    Shape {
        verb: super::friction::sub(cmd, argv).map(str::to_string),
        flags,
        npos,
    }
}

fn push(flags: &mut Vec<String>, n: &str) {
    if !flags.iter().any(|f| f == n) {
        flags.push(n.to_string());
    }
}

/// Write the shape onto a call row (reject, help and ok).
pub(crate) fn apply(cmd: &str, argv: &[String], row: &mut serde_json::Value) {
    let s = shape(cmd, argv);
    if let Some(v) = s.verb {
        row["verb"] = v.into();
    }
    row["flags"] = s.flags.into();
    row["npos"] = serde_json::json!(s.npos);
}

/// An MCP call's argument names as flag names (`urgent_before` → `urgent-before`),
/// so `stats::by_flag` counts a flag used only through MCP. Positional ones
/// (`text`, `id`) land as names too.
// ponytail: over-counts at worst, never hides a used flag from retire
pub(crate) fn mcp_flags(args: &serde_json::Value) -> Vec<String> {
    args.as_object()
        .map(|o| {
            o.keys()
                .filter(|k| *k != "verb")
                .map(|k| k.replace('_', "-"))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::shape;

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    #[test]
    fn the_briefs_reject_reads_verb_flags_and_count() {
        let s = shape("chunk", &argv("chunk show a b --json"));
        assert_eq!(s.verb.as_deref(), Some("show"));
        assert_eq!(s.flags, ["json"]);
        assert_eq!(s.npos, 2);
    }

    #[test]
    fn mcp_args_count_as_flag_names() {
        let a = serde_json::json!({"text": "x", "urgent_before": "01M", "verb": "start"});
        assert_eq!(super::mcp_flags(&a), ["text", "urgent-before"]);
    }

    #[test]
    fn flag_values_are_skipped_not_counted() {
        let s = shape("find", &argv("find --files src/a.rs --json --files b.rs"));
        assert_eq!(s.verb, None);
        assert_eq!(s.flags, ["files", "json"]);
        assert_eq!(s.npos, 0);
    }

    #[test]
    fn inline_values_stay_inline() {
        let s = shape("add", &argv("add note hi --title=t --files a.rs"));
        assert_eq!(s.flags, ["files", "title"]);
        assert_eq!(s.npos, 2);
    }

    #[test]
    fn after_dashdash_everything_is_positional() {
        let s = shape("add", &argv("add note --files a.rs -- -h"));
        assert_eq!(s.flags, ["files"]);
        assert_eq!(s.npos, 2);
    }

    #[test]
    fn an_unknown_verb_holds_the_seat_but_names_nothing() {
        let s = shape("chunk", &argv("chunk list --json"));
        assert_eq!(s.verb, None);
        assert_eq!(s.flags, ["json"]);
        assert_eq!(s.npos, 0);
    }

    #[test]
    fn help_prefix_and_bare_help_read_as_shape() {
        let s = shape("find", &argv("help find"));
        assert_eq!((s.verb, s.flags.len(), s.npos), (None, 0, 0));
        let s = shape("?", &argv("--help"));
        assert_eq!(s.flags, ["help"]);
        assert_eq!(s.npos, 0);
    }

    #[test]
    fn revisit_takes_its_value_only_when_bare() {
        let s = shape("find", &argv("find --revisit tomorrow"));
        assert_eq!(s.flags, ["revisit"]);
        assert_eq!(s.npos, 0);
        let s = shape("find", &argv("find --revisit --json"));
        assert_eq!(s.flags, ["json", "revisit"]);
        assert_eq!(s.npos, 0);
    }

    #[test]
    fn no_value_word_is_ever_kept() {
        // every kept string must be the verb or a flag name, never an operand
        for (cmd, a) in [
            ("chunk", "chunk show 01M4AAAA secret-text --json"),
            ("add", "add note login loops --title t --files src/a.rs"),
            ("find", "find --files src/a.rs --bogus v"),
        ] {
            let v: Vec<String> = a.split(' ').skip(2).map(String::from).collect();
            let s = shape(cmd, &argv(a));
            for keep in s.verb.into_iter().chain(s.flags) {
                assert!(!v.contains(&keep), "{a} kept {keep}");
            }
        }
    }
}
