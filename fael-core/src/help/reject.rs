//! Rejects worded once for both binaries: `fael` and `fael-crew` run an unknown flag or a wrong
//! verb through the same closest-real-thing answer, and the same `--files a b` retry.

/// (command, flags, what to say) when no real flag is near enough to name.
const NO_FLAG: &[(&str, &[&str], &str)] = &[
    ("find", &["--stale"], "open issues: fael find --kind issue"),
    (
        "find",
        &["--status", "--state"],
        "--all adds closed rows, --kind issue lists the open issues",
    ),
];

/// (command, flags it takes none of, what to say instead).
const TAKES_NO: &[(&str, &[&str], &str)] = &[
    (
        "bump",
        &["title", "files", "key", "kind", "text", "supersedes"],
        "re-file it: fael add … --supersedes <id>",
    ),
    (
        "close",
        &["to"],
        "the reason is the last word: fael close <id> \"<why>\"",
    ),
];

/// Reject with the closest real thing and the command's usage line, instead of
/// "try 'fael --help'". Deterministic and never runs a guess: near enough to
/// help is near enough to do the wrong thing.
pub fn reject(argv: &[String], e: String) -> String {
    // `fael chunk list|ls`: the verb that never was one — answer with the reads
    if let Some(m) = retype(argv, &e) {
        return m;
    }
    let cmd = argv.first().map_or("", String::as_str);
    let tail = " — try 'fael --help'";
    if let Some(flag) = e.strip_prefix("rejected: unknown flag ") {
        let flag = flag.split(' ').next().unwrap_or_default();
        return with_usage(
            cmd,
            format!("rejected: unknown flag {flag} — {}", near(cmd, flag)),
            &e,
        );
    }
    if let Some(head) = e.strip_suffix(tail).filter(|h| h.contains(" takes no --")) {
        let (name, flag) = head.split_once(" takes no --").unwrap_or_default();
        let name = name.trim_start_matches("rejected: ");
        let said = TAKES_NO
            .iter()
            .find(|(c, fs, _)| *c == name && fs.contains(&flag))
            .map(|(.., t)| format!("{head} — {t}"));
        return with_usage(name, said.unwrap_or(head.into()), &e);
    }
    if e.starts_with("rejected: unknown command") {
        let close = super::names()
            .filter(|n| crate::levenshtein(n, cmd) <= 2)
            .min_by_key(|n| crate::levenshtein(n, cmd));
        if let Some(n) = close {
            return format!("rejected: unknown command {cmd:?} — did you mean {n}?");
        }
    }
    e
}

/// `--files a.rs b.rs`: the paths after the first became positionals and the
/// parser said "wrong arguments" — the most common add reject (23 of 23 seen
/// 2026-10-14..17). The argv with each path-shaped word after a list flag's
/// value joined into its comma list; `None` when there is none. Called only
/// on that arity reject, so an argv that already runs never changes meaning.
pub fn join_lists(argv: &[String]) -> Option<Vec<String>> {
    let pathy =
        |w: &str| !w.starts_with('-') && !w.contains(char::is_whitespace) && w.contains(['/', '.']);
    let (mut out, mut joined, mut list) = (Vec::<String>::new(), false, false);
    for w in argv {
        // `list`: the word before this one is a list flag's value (or a word joined into it)
        if list
            && pathy(w)
            && let Some(last) = out.last_mut()
            && pathy(last)
        {
            last.push(',');
            last.push_str(w);
            joined = true;
            continue;
        }
        list = out
            .last()
            .is_some_and(|f| matches!(f.as_str(), "--files" | "--scope"));
        out.push(w.clone());
    }
    joined.then_some(out)
}

/// `msg` plus the command's synopsis line; `old` when the command has none.
pub fn with_usage(cmd: &str, msg: String, old: &str) -> String {
    match super::for_command(cmd).and_then(|s| s.lines().next()) {
        Some(u) => format!("{msg}\nusage: {u}"),
        None => old.into(),
    }
}

/// The real flags of `cmd`, hidden ones included: the full synopsis line
/// names them, plus `--json`. Chunk 3 trims the shown help to the core, so
/// reading the shown line here would lose `--groups` and friends.
fn real_flags(cmd: &str) -> Vec<&'static str> {
    let line = super::for_command_full(cmd).and_then(|s| s.lines().next());
    let mut v: Vec<&str> = line
        .unwrap_or_default()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .filter(|w| w.starts_with("--") && w.len() > 2)
        .collect();
    // the synopsis may already name it mid-line (stats does): pushing blind
    // suggests it twice, and dedup only folds neighbours.
    if !v.contains(&"--json") {
        v.push("--json");
    }
    v
}

/// What to say after an unknown `flag` on `cmd`: the real flags it could mean,
/// a pointer from the table, else the help.
fn near(cmd: &str, flag: &str) -> String {
    let real = real_flags(cmd);
    let name = flag.trim_start_matches('-');
    let mut c: Vec<&str> = match flag.starts_with("--") {
        // a typo or a prefix of a real flag (`--group` → `--groups`)
        true => real
            .iter()
            .copied()
            .filter(|r| {
                let r = &r[2..];
                crate::levenshtein(r, name) <= 1
                    || (name.len() >= 3 && (r.starts_with(name) || name.starts_with(r)))
            })
            .collect(),
        // `-k`: every real flag that letter could start — never picked for them
        false => real
            .iter()
            .copied()
            .filter(|r| r[2..].starts_with(name))
            .collect(),
    };
    c.sort_by_key(|r| crate::levenshtein(&r[2..], name));
    c.truncate(3);
    let pointer = NO_FLAG
        .iter()
        .find(|(k, fs, _)| *k == cmd && fs.contains(&flag))
        .map(|(.., t)| (*t).to_string());
    match (c.as_slice(), pointer) {
        ([], Some(t)) => t,
        ([], None) => format!("try 'fael {cmd} --help'"),
        ([one], _) => format!("did you mean {one}?"),
        ([a @ .., z], _) => format!("did you mean {} or {z}?", a.join(", ")),
    }
}

/// The retype for `fael chunk list|ls …`, or `None` for any other argv or error.
fn retype(argv: &[String], e: &str) -> Option<String> {
    let (cmd, verb) = match argv {
        [c, v, ..] => (c.as_str(), v.as_str()),
        _ => return None,
    };
    if cmd != "chunk" || !matches!(verb, "list" | "ls") {
        return None;
    }
    if !e.contains("try 'fael chunk --help'") {
        return None;
    }
    Some(format!(
        "rejected: fael chunk {verb} — fael board --plan <plan> lists a plan's chunks · fael plan export <plan> their briefs"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    #[test]
    fn near_names_the_closest_and_never_picks() {
        assert_eq!(near("find", "-k"), "did you mean --key or --kind?");
        assert_eq!(near("find", "--group"), "did you mean --groups?");
        assert_eq!(
            near("find", "--stale"),
            "open issues: fael find --kind issue"
        );
        assert_eq!(near("find", "--why"), "try 'fael find --help'");
        assert!(near("find", "--status").contains("--all"));
        // stats' synopsis already names --json: still suggested once
        assert_eq!(near("stats", "-j"), "did you mean --json?");
    }

    /// A reject that is not ours, or has no command to read usage from, goes out
    /// as it came — never cut, never crashing.
    #[test]
    fn reject_leaves_what_it_cannot_improve() {
        let flag_first = "rejected: unknown flag --stale — try 'fael --help'";
        assert_eq!(
            reject(&argv("--json find --stale"), flag_first.into()),
            flag_first
        );
        // another module's own "takes no" line, which does not end in our tail
        let auto = "rejected: `upgrade --auto` takes no --dry-run or --client";
        assert_eq!(reject(&argv("upgrade --auto --dry-run"), auto.into()), auto);
        // a command with no help section, an empty argv, an error of another kind
        assert_eq!(reject(&argv("zzz --x"), flag_first.into()), flag_first);
        assert_eq!(reject(&[], flag_first.into()), flag_first);
        let other = "rejected: --limit 0 shows nothing";
        assert_eq!(reject(&argv("find --limit 0"), other.into()), other);
        // an unknown command far from every name has no "did you mean"
        let far = "rejected: unknown command \"qqqqqqq\" — try 'fael --help'";
        assert_eq!(reject(&argv("qqqqqqq"), far.into()), far);
        // same input, same answer
        let a = reject(&argv("find --group"), flag_first.into());
        assert_eq!(a, reject(&argv("find --group"), flag_first.into()));
    }

    /// `--files a.rs b.rs c.md` joins into one comma list up to the next flag or
    /// word that is not a path; an argv with no spread list is `None`.
    #[test]
    fn a_list_flag_split_by_a_space_is_joined() {
        let got = join_lists(&argv(
            "add issue x --files a/b.rs c/d.rs AGENTS.md --key k:t",
        ));
        assert_eq!(
            got,
            Some(argv(
                "add issue x --files a/b.rs,c/d.rs,AGENTS.md --key k:t"
            ))
        );
        let got = join_lists(&argv("add note --files a.rs b.rs x"));
        assert_eq!(got, Some(argv("add note --files a.rs,b.rs x")));
        let got = join_lists(&argv("chunk edit u --scope a/ b/c.rs"));
        assert_eq!(got, Some(argv("chunk edit u --scope a/,b/c.rs")));
        // one path, a text after it, a path that is not after a list flag
        assert_eq!(join_lists(&argv("add note --files a.rs x y")), None);
        assert_eq!(join_lists(&argv("add note a.rs b.rs")), None);
    }
}
