use super::{closed, superseded};
use crate::{Log, Row};

/// Estimated tokens — derived at read time, never stored (every model's tokenizer differs).
/// An id-like run (≥ 6 chars of `0-9A-Z` mixing digits and letters — a ULID
/// prefix) costs ~1 token per 2 chars: tokenizers split random base32 far
/// finer than prose, and ids ride on every pushed row.
// est is the anchor unit (like USD with an exchange table): ASCII ≈ 4 bytes/token,
// anything else (Thai) ≈ 1 char/token — see the est → Claude 5 / Haiku 4.5 / o200k
// rates in docs/architecture.md §5; clients with `usage` get `real_tokens` instead
pub fn est_tokens(s: &str) -> usize {
    let (mut ascii, mut other, mut ids) = (0usize, 0usize, 0usize);
    let mut run = String::new();
    let flush = |run: &mut String, ascii: &mut usize, ids: &mut usize| {
        let b = run.as_bytes();
        let id_like = b.len() >= 6
            && b.iter().any(u8::is_ascii_digit)
            && b.iter().any(u8::is_ascii_uppercase);
        if id_like {
            *ids += b.len().div_ceil(2);
        } else {
            *ascii += b.len();
        }
        run.clear();
    };
    for c in s.chars() {
        if c.is_ascii_digit() || c.is_ascii_uppercase() {
            run.push(c);
            continue;
        }
        flush(&mut run, &mut ascii, &mut ids);
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    flush(&mut run, &mut ascii, &mut ids);
    ascii.div_ceil(4) + other + ids
}

/// Per-row shortest unique id prefix (≥ 8), git-style: a row is only as long
/// as its nearest neighbour forces it, so one same-second pair no longer
/// widens every id in the log. Build once per render — lookups are a binary
/// search over the sorted ids.
pub struct Abbrev(Vec<String>);

/// The ids of `log`, ready to shorten — what `render` prints.
pub fn abbrev(log: &Log) -> Abbrev {
    let mut ids: Vec<String> = log.rows.iter().map(|r| r.id.clone()).collect();
    ids.sort_unstable();
    ids.dedup();
    Abbrev(ids)
}

impl Abbrev {
    /// Also keep `id` unique against a row not yet in the log (the one `add`
    /// is about to write, maybe in the same millisecond).
    pub fn with(mut self, id: &str) -> Self {
        if let Err(i) = self.0.binary_search_by(|x| x.as_str().cmp(id)) {
            self.0.insert(i, id.to_string());
        }
        self
    }

    /// `id` cut to its shortest unique prefix (≥ 8) — the one way to print
    /// an id a user can paste back into `--supersedes`/`close`: unique now,
    /// never hand-sliced.
    pub fn short<'a>(&self, id: &'a str) -> &'a str {
        let i = self
            .0
            .binary_search_by(|x| x.as_str().cmp(id))
            .unwrap_or_else(|i| i);
        let common = |o: Option<&String>| {
            o.filter(|o| o.as_str() != id).map_or(0, |o| {
                o.bytes()
                    .zip(id.bytes())
                    .take_while(|(a, b)| a == b)
                    .count()
                    + 1
            })
        };
        let prev = i.checked_sub(1).and_then(|p| self.0.get(p));
        let next = self
            .0
            .get(i)
            .filter(|x| x.as_str() != id)
            .or(self.0.get(i + 1));
        let w = common(prev).max(common(next)).max(8);
        id.get(..w).unwrap_or(id)
    }
}

/// What a paged list's cut line needs: the pre-page total, the rows skipped
/// before this page, and how to print the exact next call from the next
/// offset. CLI passes `|n| format!("fael find --kind issue --limit 2 --offset {n}")`
/// (same flags, new offset); MCP passes `|n| format!("offset={n}")`.
pub struct Cut<'a> {
    pub total: usize,
    pub offset: usize,
    pub next: &'a dyn Fn(usize) -> String,
}

/// One markdown line per row — `- [id] kind #key title → files` — stopping once `budget`
/// estimated tokens are used (the first row always shows). A last line counts what was cut.
pub fn render(log: &Log, rows: &[&Row], budget: usize) -> String {
    render_inner(log, rows, budget, false, None)
}

/// Paged `render`: the cut line prints the exact next call instead of
/// "narrow the filter", so the agent pages without guessing. `total`/`offset`
/// come from `page()`; the next offset is this page's offset plus the rows
/// actually shown (a budget cut shortens the page, the offset follows it).
pub fn render_page(log: &Log, rows: &[&Row], budget: usize, cut: Cut) -> String {
    render_inner(log, rows, budget, false, Some(cut))
}

/// The line shows the title (`Row::display_title`), never the body — bodies come
/// back via `render_full` (`find <id>`, `--full`).
pub fn render_full(log: &Log, rows: &[&Row], budget: usize) -> String {
    render_inner(log, rows, budget, true, None)
}

/// Paged `render_full` — same cut line as `render_page`.
pub fn render_full_page(log: &Log, rows: &[&Row], budget: usize, cut: Cut) -> String {
    render_inner(log, rows, budget, true, Some(cut))
}

fn render_inner(log: &Log, rows: &[&Row], budget: usize, full: bool, cut: Option<Cut>) -> String {
    let ab = abbrev(log);
    let (closed, superseded) = (closed(log), superseded(log));
    let mut out = String::new();
    let mut used = 0;
    let mut cut_budget = false;
    // a shown title that hides part of its body — the agent is told how to read it
    let mut hidden = false;
    for (i, r) in rows.iter().enumerate() {
        let id = ab.short(&r.id);
        let mark = if closed.contains(r.id.as_str()) {
            " (closed)"
        } else if superseded.contains(r.id.as_str()) {
            " (superseded)"
        } else {
            ""
        };
        let key = r.key.as_ref().map(|k| format!(" #{k}")).unwrap_or_default();
        // `(urgent 1, to: ploy)` — whichever of the two is set, urgent first
        let route = match (r.urgent_value().map(|u| format!("urgent {u}")), r.to_who()) {
            (Some(u), Some(t)) => format!(" ({u}, to: {t})"),
            (Some(u), None) => format!(" ({u})"),
            (None, Some(t)) => format!(" (to: {t})"),
            (None, None) => String::new(),
        };
        let text = r.display_title();
        let line = format!(
            "- [{id}] {}{mark}{key} {text}{route} → {}\n",
            r.kind,
            r.files.join(", ")
        );
        // bodies read on demand only: the indented full text under its title line
        let line = if full {
            let body = r.text.split_whitespace().collect::<Vec<_>>().join(" ");
            format!("{line}  {body}\n")
        } else {
            line
        };
        used += est_tokens(&line);
        if i > 0 && used > budget {
            cut_budget = true;
            let shown = i;
            let rest = rows.len() - i;
            match &cut {
                Some(c) => out.push_str(&cut_line(
                    c.total.saturating_sub(c.offset + shown),
                    (c.next)(c.offset + shown),
                )),
                None => out.push_str(&format!(
                    "… +{rest} more over the {budget}-token budget — narrow the filter\n",
                )),
            }
            break;
        }
        hidden |= !full && text != r.text.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push_str(&line);
    }
    // a `--limit` page ends before the matches do — same line shape, no budget involved
    if !cut_budget && let Some(c) = &cut {
        let rest = c.total.saturating_sub(c.offset + rows.len());
        if rest > 0 {
            out.push_str(&cut_line(rest, (c.next)(c.offset + rows.len())));
        }
    }
    if hidden {
        out.push_str(
            "bodies: fael find <id> (MCP: find id=<id>) · every body: --full (MCP: full=true)\n",
        );
    }
    out
}

/// `… +N more — next: <the exact next call>` — an agent pages without guessing.
fn cut_line(rest: usize, next: String) -> String {
    format!("… +{rest} more — next: {next}\n")
}
