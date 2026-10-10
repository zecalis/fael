//! Read a fapony `PLAN-*.md` — the grammar `fapony plan` reads, ported line by line so an
//! import picks the same next chunk (fapony `src/plan/sweep.ts`, `parallel.ts`). No regex
//! crate: each fapony pattern is a small hand matcher named after it.

/// The frontmatter keys a plan's state needs; any other key is ignored.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Front {
    pub status: Option<String>,
    pub kind: Option<String>,
    pub area: Option<String>,
    pub spec: Option<String>,
    pub blocked_by: Option<String>,
    pub parked_because: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// `[ ]`
    Open,
    /// `[x]`
    Done,
    /// `[~]`
    Dropped,
    /// any other `[?]` — not a chunk to fapony, kept so the import can say so
    Unknown,
}

/// One checkbox line of the first `##` section (the TL;DR), in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub tick: Tick,
    /// the line as written, for the markers and the label
    pub raw: String,
    /// the text after the checkbox, trimmed (an unknown line: the whole line, trimmed)
    pub text: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMd {
    /// `PLAN-<name>.md` → `<name>`, lowercased (the `plan:<name>` anchor)
    pub name: String,
    pub title: String,
    pub front: Front,
    pub items: Vec<Item>,
}

/// `None` when `file_name` is not `PLAN-<name>.md`.
pub fn parse(file_name: &str, text: &str) -> Option<PlanMd> {
    let stem = file_name.strip_suffix(".md")?;
    let name = strip_prefix_ci(stem, "PLAN-")?;
    if name.is_empty() {
        return None;
    }
    let body = strip_front(text);
    Some(PlanMd {
        name: name.to_lowercase(),
        title: body
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .unwrap_or("")
            .trim()
            .to_string(),
        front: front(text),
        items: first_section(body).lines().filter_map(item).collect(),
    })
}

fn strip_prefix_ci<'a>(s: &'a str, p: &str) -> Option<&'a str> {
    let head = s.get(..p.len())?;
    head.eq_ignore_ascii_case(p).then(|| &s[p.len()..])
}

/// fapony `FRONT`: `---\n…\n---` at the very start; the inner text.
fn front_block(text: &str) -> Option<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    let inner = rest[..end].strip_suffix('\r').unwrap_or(&rest[..end]);
    Some((inner, &rest[end + 4..]))
}

fn strip_front(text: &str) -> &str {
    front_block(text).map_or(text, |(_, after)| after)
}

fn front(text: &str) -> Front {
    let mut f = Front::default();
    // fapony reads the first 4096 chars only
    let head = text.get(..text.len().min(4096)).unwrap_or(text);
    let Some((inner, _)) = front_block(head) else {
        return f;
    };
    for line in inner.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        // `kind: unit   # tracker …` — the comment is no value
        let v = v
            .char_indices()
            .find(|&(i, c)| c == '#' && v[..i].ends_with(char::is_whitespace))
            .map_or(v, |(i, _)| &v[..i])
            .trim();
        let v = (!v.is_empty()).then(|| v.to_string());
        match k.trim() {
            "status" => f.status = v,
            "kind" => f.kind = v,
            "area" => f.area = v,
            "blocked_by" => f.blocked_by = v,
            "parked_because" => f.parked_because = v,
            // a path is cut to its name: the first token's basename
            "spec" => {
                f.spec = v.and_then(|s| {
                    let tok = s.split_whitespace().next()?;
                    Some(tok.rsplit('/').next().unwrap_or(tok).to_string())
                });
            }
            _ => {}
        }
    }
    f
}

/// fapony `firstSection`: from the first `## ` heading to the next one.
fn first_section(body: &str) -> String {
    let mut out = Vec::new();
    for line in body.lines() {
        if is_h2(line) {
            if !out.is_empty() {
                break;
            }
            out.push(line);
        } else if !out.is_empty() {
            out.push(line);
        }
    }
    out.join("\n")
}

fn is_h2(line: &str) -> bool {
    line.strip_prefix("##")
        .and_then(|r| r.chars().next())
        .is_some_and(char::is_whitespace)
}

/// `- [?] ` / `* [?] ` at the line start (after spaces): the box char and the rest.
fn checkbox(line: &str) -> Option<(char, &str)> {
    let l = line.trim_start();
    let l = l.strip_prefix(['-', '*'])?;
    let l2 = l.trim_start();
    if l2.len() == l.len() {
        return None;
    }
    let l = l2.strip_prefix('[')?;
    let mut cs = l.chars();
    let c = cs.next()?;
    let rest = cs.as_str().strip_prefix(']')?;
    rest.starts_with(char::is_whitespace).then_some((c, rest))
}

fn item(line: &str) -> Option<Item> {
    let (c, rest) = checkbox(line)?;
    let text = rest.trim();
    let tick = match c {
        // `[ ]` needs text after it (fapony `\[\s\]\s+.+$`)
        // `[ ]` needs `\s+.+` after it: one more char past the space
        c if c.is_whitespace() && rest.chars().nth(1).is_some() => Tick::Open,
        'x' | 'X' => Tick::Done,
        '~' => Tick::Dropped,
        ']' => return None,
        _ => Tick::Unknown,
    };
    let text = if tick == Tick::Unknown {
        line.trim()
    } else {
        text
    };
    Some(Item {
        tick,
        raw: line.to_string(),
        text: text.to_string(),
        label: label(line),
    })
}

/// fapony `LABEL` at the start of `s`: `[A-Za-z]{0,3}\d[A-Za-z0-9]*`; its byte length.
fn label_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    if letters > 3 || !b.get(letters).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    Some(
        letters
            + b[letters..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric())
                .count(),
    )
}

/// fapony `LEAD`: spaces, an optional `- ` bullet with an optional `[?] ` box, then `*`/`_`.
fn lead(line: &str) -> &str {
    let mut s = line.trim_start();
    if let Some(r) = s.strip_prefix(['-', '*']) {
        let r2 = r.trim_start();
        if r2.len() < r.len() {
            s = r2;
            if let Some((c, rest)) = checkbox_box(s)
                && (c.is_whitespace() || matches!(c, 'x' | 'X' | '~'))
            {
                let t = rest.trim_start();
                if t.len() < rest.len() {
                    s = t;
                }
            }
        }
    }
    s.trim_start_matches(['*', '_'])
}

fn checkbox_box(s: &str) -> Option<(char, &str)> {
    let l = s.strip_prefix('[')?;
    let mut cs = l.chars();
    let c = cs.next()?;
    Some((c, cs.as_str().strip_prefix(']')?))
}

/// fapony `chunkLabel`: "chunk 2 — …" / "**chunk F3** — …" → "2" / "F3"; else the short
/// token before the dash ("u0 — …", "pr0 (4ddb64e, #226) — …").
pub fn label(line: &str) -> Option<String> {
    let s = lead(line);
    if let Some(r) = strip_prefix_ci(s, "chunk") {
        let r = r.trim_start_matches(|c: char| c.is_whitespace() || c == '-');
        let r = r.trim_start_matches('*');
        if let Some(n) = label_len(r) {
            return Some(r[..n].to_string());
        }
    }
    let n = label_len(s)?;
    let mut r = s[n..].trim_start_matches(['*', '_']);
    // `(?:\s+\([^)]*\))*` — parenthesised notes between the label and the dash
    loop {
        let t = r.trim_start();
        if t.len() == r.len() || !t.starts_with('(') {
            break;
        }
        let Some(close) = t.find(')') else { break };
        r = &t[close + 1..];
    }
    let t = r.trim_start();
    (t.len() < r.len() && t.starts_with(['—', '–'])).then(|| s[..n].to_string())
}

/// The first `(<word>` on the line (ASCII case-insensitive) that ends at a non-word char
/// and closes with `)` — fapony `\(wip\b\s*([^)]*)\)`: the inside, trimmed.
fn paren(line: &str, word: &str, ws_after: bool) -> Option<String> {
    // ASCII lowering keeps byte offsets, so slices of `lower` index `line` too
    let lower = line.to_ascii_lowercase();
    let pat = format!("({word}");
    let mut from = 0;
    while let Some(i) = lower[from..].find(&pat) {
        let at = from + i + pat.len();
        let next = lower[at..].chars().next();
        let ok = if ws_after {
            next.is_some_and(char::is_whitespace)
        } else {
            next.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
        };
        if ok && let Some(end) = lower[at..].find(')') {
            return Some(line[at..at + end].trim().to_string());
        }
        from = at;
    }
    None
}

/// `(wip <branch>)` / `(wait <what>)` → the inside, trimmed (`""` for a bare marker).
pub fn marker(line: &str, word: &str) -> Option<String> {
    paren(line, word, false)
}

/// fapony `afterRefs`: `(after 2, vela-jobs:j4)` → `["2", "vela-jobs:j4"]`; `None` = no
/// marker (the chunk waits on the one before it), `(after —)` = waits on nothing.
pub fn after_refs(line: &str) -> Option<Vec<String>> {
    let inner = paren(line, "after", true)?;
    Some(
        inner
            .split(|c: char| c == ',' || c == '+' || c.is_whitespace())
            .map(|t| strip_chunk_word(&t.to_lowercase()))
            .filter(|t| !t.is_empty() && !matches!(t.as_str(), "—" | "-" | "none"))
            .collect(),
    )
}

/// `chunk-3` → `3`, `vela-jobs:chunk3` → `vela-jobs:3` (first occurrence, at the start or
/// right after `:`).
fn strip_chunk_word(t: &str) -> String {
    let cut = |s: &str| {
        let r = s.strip_prefix("chunk")?;
        Some(r.strip_prefix('-').unwrap_or(r).to_string())
    };
    if let Some(r) = cut(t) {
        return r;
    }
    match t.split_once(':') {
        Some((p, l)) => cut(l).map_or_else(|| t.to_string(), |r| format!("{p}:{r}")),
        None => t.to_string(),
    }
}

#[cfg(test)]
#[path = "md_tests.rs"]
mod tests;
