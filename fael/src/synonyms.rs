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

pub(crate) use fael_core::help::reject::{join_lists, reject, with_usage};

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

/// What agents type for a kind → what the ledger keeps (`bug` → `issue`).
/// Input only, like the rest of this table — never advertised.
const KIND_ALIASES: &[(&str, &str)] = &[("bug", "issue")];

/// The real kind for a guessed one.
pub(crate) fn kind(k: &str) -> &str {
    KIND_ALIASES
        .iter()
        .find(|(from, _)| *from == k)
        .map(|(_, to)| *to)
        .unwrap_or(k)
}

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

fn listed(cmds: &str, cmd: &str) -> bool {
    cmds.split(' ').any(|c| c == cmd)
}

/// The argv with every synonym in the table replaced by the real thing.
pub(crate) fn rewrite(mut argv: Vec<String>) -> Vec<String> {
    match argv.first().map(String::as_str) {
        None => return argv,
        Some("version") if argv.len() == 1 => return vec!["--version".into()],
        Some(k) if KINDS.contains(&k) => argv.insert(0, "add".into()),
        Some(k) if kind(k) != k => {
            argv[0] = kind(k).into();
            argv.insert(0, "add".into());
        }
        Some(c) => {
            if let Some((_, to)) = COMMANDS.iter().find(|(from, _)| *from == c) {
                argv[0] = (*to).into();
            }
        }
    }
    if argv.first().map(String::as_str) == Some("add") && argv.get(1).is_some_and(|k| kind(k) != k)
    {
        let real = kind(argv[1].as_str()).to_string();
        argv[1] = real;
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
    if cmd == "add" && text.is_some() {
        headline(&mut out);
    }
    // the text goes last, where `close <id> "<why>"` reads it
    out.extend(text);
    out.extend(tail);
    out
}

/// `add note "<headline>" --body "<text>"`: with no `--title`, the one text
/// before the body flag is its title — the shape a long row needs anyway, and
/// two texts the parser would only reject.
fn headline(out: &mut Vec<String>) {
    if out
        .iter()
        .any(|s| s == "--title" || s.starts_with("--title="))
    {
        return;
    }
    let Ok(a) = crate::Args::parse(out.clone()) else {
        return;
    };
    // ponytail: the last arg equal to it — a flag value spelled the same is the only miss
    if let [_, _, t] = a.pos.as_slice()
        && let Some(i) = out.iter().rposition(|s| s == t)
    {
        out.insert(i, "--title".into());
    }
}

/// MCP `arguments` with guessed property names renamed (`query` → `text`).
pub(crate) fn mcp(tool: &str, args: &Value) -> Value {
    let mut a = args.clone();
    // `bug` is an issue the agent hasn't learned the ledger name for.
    if tool == "add" {
        if let Some(k) = a.get("kind").and_then(|v| v.as_str()) {
            let real = kind(k);
            if real != k {
                a["kind"] = json!(real);
            }
        }
        if let Some(rows) = a.get_mut("rows").and_then(|v| v.as_array_mut()) {
            for r in rows {
                if let Some(k) = r.get("kind").and_then(|v| v.as_str()) {
                    let real = kind(k);
                    if real != k {
                        r["kind"] = json!(real);
                    }
                }
            }
        }
    }
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

/// What a failed run says: a text flag clash (`text_twice`), else `reject`.
/// `raw` is the agent's argv, `argv` the rewritten one.
pub(crate) fn explain(raw: &[String], argv: &[String], e: String) -> String {
    reject(argv, text_twice(raw, argv, e))
}

/// `add note "x" --body "y"`: `rewrite` moved the flag into the text slot, so
/// the parser saw two texts and said only "wrong arguments". Name the flag
/// when the rewritten argv (`argv`) has one positional more than the command
/// takes and the agent's own argv (`raw`) holds a text flag.
fn text_twice(raw: &[String], argv: &[String], e: String) -> String {
    let cmd = argv.first().map_or("", String::as_str);
    let slot = match cmd {
        "add" => "<kind>",
        "close" => "<id>",
        _ => return e,
    };
    if !e.starts_with("rejected: wrong arguments") {
        return e;
    }
    let flag = raw
        .iter()
        .map(|s| s.split('=').next().unwrap_or(s))
        .find(|s| {
            FLAGS
                .iter()
                .any(|(c, f, to)| matches!(to, Text) && f == s && listed(c, cmd))
        });
    // the command, its kind or id, and two texts
    let two = crate::Args::parse(argv.to_vec()).is_ok_and(|a| a.pos.len() > 3);
    match flag {
        Some(f) if two => with_usage(
            cmd,
            format!(
                "rejected: {f} is the text, and a text was given too — pass it once: fael {cmd} {slot} \"<text>\""
            ),
            &e,
        ),
        _ => e,
    }
}

#[cfg(test)]
mod tests;
