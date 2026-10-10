//! Positionals + flags — the one parser every command reads through.
//! Split from `main.rs` at the 400-line ratchet; `crate::Args` still resolves.

use std::collections::HashMap;

/// Positionals + `--flag value` / `--flag=value`; `--` ends flags.
pub(crate) struct Args {
    pub(crate) pos: Vec<String>,
    flags: HashMap<String, Vec<String>>,
}

/// `-k`, `-n3`: a dash and a word — not `-` (stdin), `-1`, or text with spaces.
fn is_short_flag(s: &str) -> bool {
    let mut c = s.chars();
    c.next() == Some('-')
        && c.next().is_some_and(|c| c.is_ascii_alphabetic())
        && !s.contains(char::is_whitespace)
}

impl Args {
    pub(crate) fn parse(argv: Vec<String>) -> Result<Args, String> {
        let mut a = Args {
            pos: vec![],
            flags: HashMap::new(),
        };
        let mut it = argv.into_iter().peekable();
        while let Some(s) = it.next() {
            if s == "--" {
                a.pos.extend(it.by_ref());
                break;
            }
            let Some(name) = s.strip_prefix("--") else {
                // `-k` is a flag nobody defined, never an id or a text
                if is_short_flag(&s) {
                    return Err(format!("rejected: unknown flag {s} — try 'fael --help'"));
                }
                a.pos.push(s);
                continue;
            };
            let (name, inline) = match name.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (name.to_string(), None),
            };
            // `--revisit` takes an optional value: bare on `find` means "any
            // revisit" (`find --revisit`), with a value it narrows (`add` and
            // `find --revisit=<text>`); bare on `add` is rejected there.
            if name == "revisit" {
                let v = match inline {
                    Some(v) => Some(v),
                    None if it.peek().is_some_and(|n| !n.starts_with("--")) => it.next(),
                    None => None,
                };
                a.flags.entry(name).or_default().extend(v);
                continue;
            }
            match name.as_str() {
                "all" | "force" | "json" | "dry-run" | "yes" | "replace-fapony" | "fix"
                | "prune" | "urgent" | "not-urgent" | "full" | "rows" | "branches" | "fat"
                | "day" | "pin" | "open" | "groups" | "misses" | "wiring" | "auto" => {
                    a.flags.entry(name).or_default();
                }
                "files" | "key" | "supersedes" | "kind" | "since" | "by" | "client" | "writer"
                | "before" | "map" | "to" | "from" | "title" | "urgent-before" | "limit"
                | "offset" | "text" | "edge" | "remote" | "out" | "replace" | "with" | "plan"
                | "brief" | "size" | "model" | "scope" | "after" | "run" | "on" | "until"
                | "pr" => {
                    let v = inline
                        .or_else(|| it.next())
                        .ok_or(format!("rejected: --{name} needs a value"))?;
                    a.flags.entry(name).or_default().push(v);
                }
                _ => {
                    return Err(format!(
                        "rejected: unknown flag --{name} — try 'fael --help'"
                    ));
                }
            }
        }
        Ok(a)
    }

    pub(crate) fn has(&self, f: &str) -> bool {
        self.flags.contains_key(f)
    }

    /// Reject a flag the parser knows but `cmd` never reads — `bump --dry-run`
    /// must not silently write.
    pub(crate) fn only(&self, cmd: &str, allowed: &[&str]) -> Result<(), String> {
        match self.flags.keys().find(|f| !allowed.contains(&f.as_str())) {
            Some(f) => Err(format!(
                "rejected: {cmd} takes no --{f} — try 'fael --help'"
            )),
            None => Ok(()),
        }
    }

    pub(crate) fn one(&self, f: &str) -> Option<String> {
        self.flags.get(f).and_then(|v| v.last()).cloned()
    }

    /// `--map a=b --map c=d` → all values, in order.
    pub(crate) fn many(&self, f: &str) -> Vec<String> {
        self.flags.get(f).cloned().unwrap_or_default()
    }

    /// `--files a,b --files c` → [a, b, c]
    pub(crate) fn files(&self) -> Vec<String> {
        self.flags
            .get("files")
            .into_iter()
            .flatten()
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }

    /// `--revisit` on `add`/`bump` always needs a value — bare `--revisit` only
    /// filters on `find`. Shared by the CLI entry and `bump` so the two never
    /// drift apart.
    pub(crate) fn revisit_value(&self) -> Result<Option<String>, String> {
        match (self.has("revisit"), self.one("revisit")) {
            (false, _) => Ok(None),
            (true, Some(v)) => Ok(Some(v)),
            (true, None) => Err(
                "rejected: --revisit needs a value — a date YYYY-MM[-DD] or text like \"mdl lands\""
                    .into(),
            ),
        }
    }

    /// `--limit N` / `--offset M` for pull paging (chunk 5): at most N ranked
    /// rows, skipping M first. Offset without limit pages budget cuts too.
    pub(crate) fn paging(&self) -> Result<(Option<usize>, usize), String> {
        let num = |f: &str| match self.one(f) {
            None => Ok(None),
            Some(v) => v
                .parse::<usize>()
                .map(Some)
                .map_err(|_| format!("rejected: --{f} needs a number — got {v:?}")),
        };
        let limit = num("limit")?;
        if limit == Some(0) {
            return Err("rejected: --limit 0 shows nothing — drop it or give 1 or more".into());
        }
        Ok((limit, num("offset")?.unwrap_or(0)))
    }
}
