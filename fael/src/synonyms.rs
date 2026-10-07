//! The one synonym table (PLAN-fael-agent-ergonomics chunk 2): what an agent
//! guesses → what fael calls it. `rewrite` reads it for the CLI, `mcp` for the
//! tool properties, `reject` words the failures that stay.
//!
//! Input only. Help, schema and output keep the real names, so the table is
//! never advertised — advertised, it would be one more thing to learn. Rows are
//! per command, never global (`--to` means different things); a guess that
//! fits two real flags (`-k`, `-f`) is rejected with both, never picked.

use serde_json::{Value, json};

enum To {
    Flag(&'static str),
    /// the text is the value — `close <id> --why x` is `close <id> x`
    Text,
    /// the value is a positional — `find --id x` is `find x`
    Id,
}
use To::{Flag, Id, Text};

/// (commands, guessed flag, real thing). `*` in the README of the plan: every
/// guess marked there was seen in an agent transcript.
const FLAGS: &[(&str, &str, To)] = &[
    ("find", "--id", Id),
    ("find", "--query", Flag("--text")),
    ("find", "-q", Flag("--text")),
    ("find", "--search", Flag("--text")),
    ("find", "--type", Flag("--kind")),
    ("find add", "--file", Flag("--files")),
    ("find add", "--path", Flag("--files")),
    ("find add", "--paths", Flag("--files")),
    ("find add", "--tag", Flag("--key")),
    ("find", "-n", Flag("--limit")),
    ("find", "-a", Flag("--all")),
    ("find add", "-j", Flag("--json")),
    ("close", "--why", Text),
    ("close", "--reason", Text),
    ("close", "--note", Text),
    ("close", "--body", Text),
    ("close", "-m", Text),
    ("add", "--body", Text),
    ("add", "--message", Text),
    ("add", "-m", Text),
];

const COMMANDS: &[(&str, &str)] = &[
    ("show", "find"),
    ("get", "find"),
    ("search", "find"),
    ("list", "find"),
    ("ls", "find"),
    ("done", "close"),
    ("resolve", "close"),
    ("new", "add"),
    ("create", "add"),
];

/// `fael decision "…"` is `fael add decision "…"`.
const KINDS: [&str; 3] = ["decision", "issue", "note"];

/// (tools, guessed property, real property) — the real one wins when both come.
const PROPS: &[(&str, &str, &str)] = &[
    ("find", "query", "text"),
    ("find", "search", "text"),
    ("find", "type", "kind"),
    ("find add", "file", "files"),
    ("find add", "path", "files"),
    ("find add", "paths", "files"),
    ("find add", "tag", "key"),
    ("close", "why", "text"),
    ("close", "reason", "text"),
    ("close", "note", "text"),
    ("close", "body", "text"),
    ("add", "body", "text"),
    ("add", "message", "text"),
];

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

fn listed(cmds: &str, cmd: &str) -> bool {
    cmds.split(' ').any(|c| c == cmd)
}

/// The argv with every synonym in the table replaced by the real thing.
pub(crate) fn rewrite(mut argv: Vec<String>) -> Vec<String> {
    match argv.first().map(String::as_str) {
        None => return argv,
        Some("version") if argv.len() == 1 => return vec!["--version".into()],
        Some(k) if KINDS.contains(&k) => argv.insert(0, "add".into()),
        Some(c) => {
            if let Some((_, to)) = COMMANDS.iter().find(|(from, _)| *from == c) {
                argv[0] = (*to).into();
            }
        }
    }
    if argv[0].starts_with('-') {
        return argv;
    }
    let cmd = argv[0].clone();
    let (mut out, mut text, mut tail) = (vec![], None, vec![]);
    let mut it = argv.into_iter();
    out.extend(it.next());
    while let Some(s) = it.next() {
        if s == "--" {
            tail.push(s);
            tail.extend(it.by_ref());
            break;
        }
        let (name, inline) = match s.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (s.clone(), None),
        };
        let hit = FLAGS
            .iter()
            .find(|(c, f, _)| *f == name && listed(c, &cmd))
            .map(|(.., to)| to);
        match hit {
            Some(Flag(real)) => out.push(match inline {
                Some(v) => format!("{real}={v}"),
                None => (*real).into(),
            }),
            Some(to) => match inline.or_else(|| it.next()) {
                Some(v) if matches!(to, Id) => out.push(v),
                Some(v) => text = Some(v),
                // nothing after it: leave the flag, the parser rejects it
                None => out.push(s),
            },
            None => out.push(s),
        }
    }
    // the text goes last, where `close <id> "<why>"` reads it
    out.extend(text);
    out.extend(tail);
    out
}

/// MCP `arguments` with guessed property names renamed (`query` → `text`).
pub(crate) fn mcp(tool: &str, args: &Value) -> Value {
    let mut a = args.clone();
    let Some(o) = a.as_object_mut() else {
        return a;
    };
    for (tools, from, to) in PROPS {
        if !listed(tools, tool) || o.contains_key(*to) {
            continue;
        }
        if let Some(v) = o.remove(*from) {
            // `file: "a.rs"` is a one-path list
            let v = if *to == "files" && v.is_string() {
                json!([v])
            } else {
                v
            };
            o.insert((*to).into(), v);
        }
    }
    a
}

/// Reject with the closest real thing and the command's usage line, instead of
/// "try 'fael --help'". Deterministic and never runs a guess: near enough to
/// help is near enough to do the wrong thing.
pub(crate) fn reject(argv: &[String], e: String) -> String {
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
        let close = crate::help::names()
            .filter(|n| lev(n, cmd) <= 2)
            .min_by_key(|n| lev(n, cmd));
        if let Some(n) = close {
            return format!("rejected: unknown command {cmd:?} — did you mean {n}?");
        }
    }
    e
}

/// `msg` plus the command's synopsis line; `old` when the command has none.
fn with_usage(cmd: &str, msg: String, old: &str) -> String {
    match crate::help::for_command(cmd).and_then(|s| s.lines().next()) {
        Some(u) => format!("{msg}\nusage: {u}"),
        None => old.into(),
    }
}

/// The real flags of `cmd`: the ones its synopsis line names, plus `--json`.
fn real_flags(cmd: &str) -> Vec<&'static str> {
    let line = crate::help::for_command(cmd).and_then(|s| s.lines().next());
    let mut v: Vec<&str> = line
        .unwrap_or_default()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .filter(|w| w.starts_with("--") && w.len() > 2)
        .collect();
    v.push("--json");
    v.dedup();
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
                lev(r, name) <= 1
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
    c.sort_by_key(|r| lev(&r[2..], name));
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

/// Edit distance, two rows.
fn lev(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests;
