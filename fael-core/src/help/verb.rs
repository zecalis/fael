//! One verb's help: `fael <cmd> <verb> --help` prints only that verb's
//! synopsis line(s), one ready example, and a pointer back to the full list.
//! The synopses come from the same COMMANDS table `for_command_full` reads,
//! so this file owns no help text — only the extraction and the examples.

/// `fael chunk approve --help` (or `fael help chunk approve`): the verb's
/// help, or None when this argv is not a verb help (unknown verb falls back
/// to the full command section in `help::for_argv`). `--all` before `--` is
/// never verb help: it asks for the full section, so it returns None too.
pub fn for_argv(argv: &[String]) -> Option<String> {
    if !super::is_request(argv) || argv.iter().take_while(|x| *x != "--").any(|x| x == "--all") {
        return None;
    }
    let words: Vec<&str> = argv
        .iter()
        .take_while(|x| *x != "--")
        .filter(|a| !a.starts_with('-') && a.as_str() != "help")
        .map(String::as_str)
        .collect();
    if words.len() < 2 {
        return None;
    }
    for_verb(words[0], words[1])
}

/// Only the synopsis invocation(s) naming `verb`, plus one example and the
/// pointer back. Unknown verbs (or commands) yield None.
pub fn for_verb(cmd: &str, verb: &str) -> Option<String> {
    let section = super::for_command_full(cmd)?;
    let mut hits: Vec<String> = invocations(section, cmd)
        .into_iter()
        .filter(|inv| mentions(inv, verb))
        .collect();
    if hits.is_empty() {
        return None;
    }
    hits.truncate(4);
    let first = hits[0].clone();
    for line in &hits {
        debug_assert!(line.contains(verb));
    }
    let mut out = hits;
    out.push(format!("eg: {}", example(cmd, verb).unwrap_or(&first)));
    out.push(format!("all verbs: fael {cmd} --help"));
    Some(out.join("\n"))
}

/// The first synopsis line naming `verb` — what a wrong-arguments reject shows
/// instead of the whole verb list.
pub fn synopsis(cmd: &str, verb: &str) -> Option<String> {
    let section = super::for_command_full(cmd)?;
    invocations(section, cmd)
        .into_iter()
        .find(|inv| mentions(inv, verb))
}

/// Synopsis invocations of `cmd`: every `fael <cmd> …` fragment, with `|` and
/// `·` separators between invocations kept apart from `|` inside one verb
/// alternative (`park|unpark|done`). The `<verb>` template line is skipped.
fn invocations(section: &str, cmd: &str) -> Vec<String> {
    let needle = format!("fael {cmd}");
    let mut groups = Vec::new();
    for line in section.lines().filter(|l| l.contains(&needle)) {
        for dot in split_top(line, '·') {
            let mut cur: Option<String> = None;
            for part in split_top(&dot, '|') {
                let p = part.trim();
                if p.is_empty() {
                    continue;
                }
                if p.contains(&needle) {
                    if let Some(c) = cur.take() {
                        groups.push(c);
                    }
                    cur = Some(p.to_string());
                } else if let Some(c) = cur.as_mut() {
                    c.push('|');
                    c.push_str(p);
                }
            }
            if let Some(c) = cur.take() {
                groups.push(c);
            }
        }
    }
    groups
        .into_iter()
        .map(|g| g.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|g| !g.contains("<verb>"))
        .collect()
}

/// Split on `delim` only outside `()`, `[]`, `{}`, `<>` and double quotes —
/// the `·` inside `apply`'s `(stdin: …)` and the `|` inside `<file|->` are
/// not invocation separators.
fn split_top(s: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut in_quotes = false;
    for c in s.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
            cur.push(c);
            continue;
        }
        if !in_quotes {
            match c {
                '(' | '[' | '{' | '<' => depth += 1,
                ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
                d if d == delim && depth == 0 => {
                    parts.push(std::mem::take(&mut cur));
                    continue;
                }
                _ => {}
            }
        }
        cur.push(c);
    }
    parts.push(cur);
    parts
}

/// `verb` as a whole word in `frag` — `park` matches `park|…` but not the
/// `park` inside `unpark`, and `approve` does not match `unapprove`.
fn mentions(frag: &str, verb: &str) -> bool {
    if verb.is_empty() || verb.len() > frag.len() {
        return false;
    }
    let (f, v) = (frag.as_bytes(), verb.as_bytes());
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-';
    (0..=f.len() - v.len()).any(|i| {
        &f[i..i + v.len()] == v
            && (i == 0 || !word(f[i - 1]))
            && (i + v.len() == f.len() || !word(f[i + v.len()]))
    })
}

/// One ready example per verb; unknown pairs fall back to the synopsis line.
fn example(cmd: &str, verb: &str) -> Option<&'static str> {
    match (cmd, verb) {
        ("chunk", "add") => {
            Some("fael chunk add \"rerank rows\" --plan inbox --brief \"why + done\" --size S")
        }
        ("chunk", "show") => Some("fael chunk show 01M582E4V1RQR033P5V00B4WVV"),
        ("chunk", "start") => Some("fael chunk start 01M582E4V1RQR033P5V00B4WVV"),
        ("chunk", "edit") => {
            Some("fael chunk edit 01M582E4V1RQR033P5V00B4WVV --brief \"new brief\"")
        }
        ("chunk", "move") => Some("fael chunk move 01M582E4V1RQR033P5V00B4WVV --plan other-plan"),
        ("chunk", "note") => {
            Some("fael chunk note 01M582E4V1RQR033P5V00B4WVV \"verbs done; next examples\"")
        }
        ("chunk", "touch") => Some("fael chunk touch 01M582E4V1RQR033P5V00B4WVV src/a.rs"),
        ("chunk", "wait") => Some(
            "fael chunk wait 01M582E4V1RQR033P5V00B4WVV \"which API?\" --on owner --option \"A\" --option \"B\"",
        ),
        ("chunk", "after") => Some(
            "fael chunk after 01M582E4V1RQR033P5V00B4WVV 01M582E4V1RQR033P5V00B4WVV \"needs API first\"",
        ),
        ("chunk", "unafter") => Some(
            "fael chunk unafter 01M582E4V1RQR033P5V00B4WVV 01M582E4V1RQR033P5V00B4WVV \"no longer blocked\"",
        ),
        ("chunk", "done") => Some("fael chunk done \"verbs ship\" --left none"),
        ("chunk", "answer") => {
            Some("fael chunk answer 01M582E4V1RQR033P5V00B4WVV \"the count is 210\"")
        }
        ("chunk", "drop") => Some("fael chunk drop 01M582E4V1RQR033P5V00B4WVV \"out of scope\""),
        ("chunk", "park") => {
            Some("fael chunk park 01M582E4V1RQR033P5V00B4WVV \"waiting on owner\"")
        }
        ("chunk", "accept") => Some("fael chunk accept 01M582E4V1RQR033P5V00B4WVV"),
        ("chunk", "unpark") => Some("fael chunk unpark 01M582E4V1RQR033P5V00B4WVV"),
        ("chunk", "apply") => Some(
            "echo '[{\"op\":\"split\",\"chunk\":\"a\",\"into\":[{\"title\":\"a1\",\"label\":\"b1\",\"size\":\"S\",\"scope\":[\"src/\"]},{\"title\":\"a2\"}]}]' | fael chunk apply --json -",
        ),
        ("chunk", "approve") => Some("fael chunk approve 01M582E4V1RQR033P5V00B4WVV --model small"),
        ("chunk", "unapprove") => Some("fael chunk unapprove 01M582E4V1RQR033P5V00B4WVV"),
        ("plan", "new") => Some("fael plan new my-plan \"My title\""),
        ("plan", "list") => Some("fael plan list"),
        ("plan", "export") => Some("fael plan export my-plan"),
        ("plan", "apply") => Some("fael plan apply bill.json --dry-run"),
        ("plan", "park") => Some("fael plan park my-plan"),
        ("plan", "unpark") => Some("fael plan unpark my-plan"),
        ("plan", "done") => Some("fael plan done my-plan"),
        ("plan", "undone") => Some("fael plan undone my-plan"),
        ("plan", "drop") => Some("fael plan drop my-plan"),
        ("plan", "lint") => Some("fael plan lint my-plan"),
        ("run", "end") => Some("fael run end R123"),
        ("run", "beat") => Some("fael run beat R123"),
        _ => None,
    }
}
