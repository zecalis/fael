use super::Filter;
use super::{est_tokens, glob};
use crate::{Config, Log, Row, anchor, is_carrier_row};
use std::collections::HashMap;

/// A row by exact id or a unique prefix (like a git sha, case-insensitive).
pub fn resolve<'a>(log: &'a Log, prefix: &str) -> Result<&'a Row, String> {
    resolve_among(log.rows.iter(), prefix)
}

/// [`resolve`] over content rows only — what `close` and `bump` act on. A
/// carrier (bump, restore) is never their target, and a bump event filed
/// right after its row shares the prefix an earlier output printed.
pub fn resolve_row<'a>(log: &'a Log, prefix: &str) -> Result<&'a Row, String> {
    resolve_among(log.rows.iter().filter(|r| !is_carrier_row(r)), prefix)
}

fn resolve_among<'a>(
    rows: impl Iterator<Item = &'a Row> + Clone,
    prefix: &str,
) -> Result<&'a Row, String> {
    if let Some(r) = rows.clone().find(|r| r.id == prefix) {
        return Ok(r);
    }
    let hits: Vec<&Row> = rows
        .filter(|r| {
            !prefix.is_empty()
                && r.id
                    .get(..prefix.len())
                    .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
        })
        .collect();
    match hits.as_slice() {
        [r] => Ok(r),
        [] => Err(format!(
            "rejected: no row with id {prefix:?} — copy the id from fael find (this session's output, never from memory)"
        )),
        _ => Err(format!(
            "rejected: id {prefix:?} matches {} rows ({}) — use more characters",
            hits.len(),
            hits.iter()
                .take(5)
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// One key in use: how many rows carry it and the `ts` of the newest.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyUse {
    pub key: String,
    pub count: usize,
    pub last: String,
}

/// Every key matching `pattern` (all when `None`), most used first — so agents reuse one.
pub fn keys(log: &Log, pattern: Option<&str>) -> Vec<KeyUse> {
    let mut out = tally(log.rows.iter().filter(|r| {
        r.key
            .as_deref()
            .is_some_and(|k| pattern.is_none_or(|p| glob(p, k)))
    }));
    out.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));
    out
}

/// One `KeyUse` per key the rows carry; keyless rows are skipped.
pub(crate) fn tally<'a>(rows: impl Iterator<Item = &'a Row>) -> Vec<KeyUse> {
    let mut by: HashMap<&str, (usize, &Row)> = HashMap::new();
    for r in rows {
        let Some(k) = r.key.as_deref() else { continue };
        let e = by.entry(k).or_insert((0, r));
        e.0 += 1;
        if r.id > e.1.id {
            e.1 = r;
        }
    }
    by.into_iter()
        .map(|(k, (count, r))| KeyUse {
            key: k.into(),
            count,
            last: r.ts.clone(),
        })
        .collect()
}

/// No filter = the session brief under the kickoff budget; otherwise find under the find budget.
/// Either way the ranked list is paged (`Filter::limit`/`offset`) before it
/// reaches render. The token budget caps a call that names no `limit`; a
/// caller that names one asked for that many rows, so the limit wins.
pub fn query<'a>(log: &'a Log, f: &Filter, cfg: &Config) -> (Vec<&'a Row>, usize, usize) {
    let (rows, budget) = if f.is_empty() && !f.all {
        (super::brief(log, f), cfg.kickoff_tokens)
    } else {
        (super::find(log, f), cfg.find_tokens)
    };
    // issues ready to work first, those waiting on a revisit after — stable,
    // so each half keeps its rank (the vela "WHEN: …" issues mixed in)
    let rows = if f.kind.as_deref() == Some("issue") {
        let day = super::revisit::today();
        let (wait, ready): (Vec<_>, Vec<_>) = rows
            .into_iter()
            .partition(|r| super::revisit::row_waiting(r, &day));
        ready.into_iter().chain(wait).collect()
    } else {
        rows
    };
    let (page, total) = super::page(rows, f.limit, f.offset);
    (page, f.limit.map_or(budget, |_| usize::MAX), total)
}

/// Chars per clause below which `;`-separated chunks read as a topic list
/// rather than prose sentences: three-plus separators warn only while the
/// row stays short (`chars < seps * CHARS_PER_CLAUSE`). Calibrated against
/// real ledger rows (604–949 chars, 3–5 seps, all single-topic, all silent).
/// Char-based so Thai/CJK (no spaces) judge by the same ruler as English.
const CHARS_PER_CLAUSE: usize = 100;

/// The chunk-3 fat-row conditions as reason bodies (no `warning: ` prefix):
/// a decision with no key, `·`/`;` joining topics, doc paths with no
/// anchor, text over `warn.row_tokens`.
/// `warnings` renders these at add time; `doctor [Fat]` reuses them for open
/// rows — one function so the two never drift apart.
pub fn fat_reasons(row: &Row, cfg: &Config) -> Vec<String> {
    let mut r = vec![];
    // a decision with no key is hard to find and to supersede alone —
    // notes and issues stay keyless without complaint
    if row.kind == "decision" && row.key.as_deref().is_none_or(|k| k.trim().is_empty()) {
        r.push(
            "decision has no --key — add --key area:topic so it can be found and superseded alone"
                .into(),
        );
    }
    let chars = row.text.chars().count();
    // `·` joins topics — two of them is a list of topics. `;` is also plain
    // English clause punctuation inside one topic: three separators read as
    // a topic list only while the clauses stay short — prose clauses run a
    // sentence long, so the check is a density and a long row needs more
    // `;` per char before it stops being prose
    let mid = row.text.chars().filter(|&c| c == '·').count();
    let seps = mid + row.text.chars().filter(|&c| c == ';').count();
    if mid >= 2 || (seps >= 3 && chars < seps * CHARS_PER_CLAUSE) {
        r.push(format!(
            "text has {seps} topic separators (; / ·) — one topic per row: split it, \
each with --key area:topic, so one can be superseded alone"
        ));
    }
    // a condition written into the text lists as ready work: --revisit is
    // the field `find --kind issue` sorts waiting rows by. Warn, never parse.
    if row.text.contains("WHEN:") && row.revisit().is_none_or(|v| v.trim().is_empty()) {
        r.push(
            "text names a condition (WHEN:) but no --revisit — add --revisit \"<condition or YYYY-MM-DD>\" so find lists it as waiting, not ready".into(),
        );
    }
    // a row whose real files are all docs and carries no anchor leaves
    // kickoff the day the docs move or go (`gone` keeps anchors, never
    // paths — see gone_files): nudge to name the code it is about, or an
    // anchor that never moves
    let real: Vec<&str> = row
        .files
        .iter()
        .map(String::as_str)
        .filter(|f| anchor(f).is_none())
        .collect();
    if !real.is_empty() && row.files.len() == real.len() && real.iter().all(|f| f.ends_with(".md"))
    {
        // the anchor ready to paste, so a row only a doc can cite (product
        // direction, a plan) gets it right the first time
        let anchors: Vec<String> = real.iter().map(|f| md_anchor(f)).collect();
        r.push(format!(
            "files name only *.md docs and no anchor — this row leaves kickoff when the docs move or go; add the code it is about, or an anchor: --files {},{}",
            row.files.join(","),
            anchors.join(",")
        ));
    }
    // a long row is usually several decisions in one: reversing one then
    // means superseding them all, so the nudge is to split, not to trim
    let t = est_tokens(&row.text);
    let (over_t, over_c) = (t > cfg.warn_row_tokens, chars > cfg.warn_row_chars);
    if over_t || over_c {
        // name the limit that actually tripped — a row can pass the token
        // estimate yet run long, and blaming the token limit reads as a false
        // alarm the agent learns to ignore
        let limit = match (over_t, over_c) {
            (true, true) => format!(
                "~{t} tokens (warn at {}) and {chars} chars (warn at {})",
                cfg.warn_row_tokens, cfg.warn_row_chars
            ),
            (true, false) => format!("~{t} tokens (warn at {})", cfg.warn_row_tokens),
            (false, true) => format!("{chars} chars (warn at {})", cfg.warn_row_chars),
            (false, false) => unreachable!(),
        };
        r.push(format!(
            "text is {limit} — one topic per row: split it, each with --key area:topic, \
             so one can be superseded alone (every push costs the full text)"
        ));
    }
    r
}

/// The stem of a `plan:<name>:chunk-<n>` or `plan:<name>:handoff` key
/// (format.md §Plan keys): `Some("plan:<name>")` when the last segment is
/// `chunk-<digits>` or `handoff`, else `None`. Per-chunk rows are sequential
/// by convention and the handoff sits beside them — chunk-1 vs chunk-3 is
/// the next chunk, not a typo — so `warnings` never reports two keys with
/// the same stem as similar.
fn chunk_stem(k: &str) -> Option<&str> {
    let (stem, tail) = k.rsplit_once(':')?;
    if tail == "handoff" {
        return Some(stem);
    }
    let n = tail.strip_prefix("chunk-")?;
    (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some(stem)
}

/// Warnings for a row about to be added — never a reject: a key domain the repo did not declare,
/// a new key close to an existing one, text over `warn.row_tokens`.
/// The anchor a doc stands for: `PLAN-vela.md` → `plan:vela` (the plan
/// convention, AGENTS.md), any other `PRODUCT.md` → `doc:product`.
fn md_anchor(f: &str) -> String {
    let stem = f.rsplit('/').next().unwrap_or(f).trim_end_matches(".md");
    let lower = stem.to_ascii_lowercase();
    match lower.strip_prefix("plan-") {
        Some(name) => format!("plan:{name}"),
        None => format!("doc:{lower}"),
    }
}

pub fn warnings(row: &Row, log: &Log, cfg: &Config) -> Vec<String> {
    let mut w = vec![];
    if let Some(k) = &row.key {
        let domain = k.split(':').next().unwrap_or(k);
        if !cfg.key_domains.is_empty() && !cfg.key_domains.iter().any(|d| d == domain) {
            w.push(format!(
                "warning: key domain {domain:?} is not in config key_domains ({}) — reuse one if it fits",
                cfg.key_domains.join(", ")
            ));
        }
        let used = keys(log, None);
        if !used.iter().any(|u| &u.key == k) {
            let parent = |s: &str| s.rsplit_once(':').map(|(p, _)| p.to_string());
            // share a parent only when both keys go at least three levels
            // deep: with two-level area:topic keys the parent is the bare
            // domain, which would warn every new topic under it — domain
            // reuse is the key_domains check above, topic typos are
            // levenshtein's job
            let deep = |s: &str| s.split(':').count() >= 3;
            let similar: Vec<&str> = used
                .iter()
                .map(|u| u.key.as_str())
                .filter(|u| {
                    // same-stem chunk keys are sequential handoffs, not typos:
                    // suggesting reuse here fires on every chunk and teaches
                    // agents to ignore the warning
                    if let (Some(a), Some(b)) = (chunk_stem(u), chunk_stem(k))
                        && a == b
                    {
                        return false;
                    }
                    (deep(u) && deep(k) && parent(u) == parent(k)) || levenshtein(u, k) <= 2
                })
                .take(5)
                .collect();
            if !similar.is_empty() {
                w.push(format!(
                    "warning: new key {k:?}, similar keys exist: {} — reuse one if it means the same",
                    similar.join(", ")
                ));
            }
        }
    }
    let mut shape = shape_warnings(row, cfg);
    // a re-file carries the old row's shape: repeating what the old row
    // already had teaches agents to skip warnings, so only what is new is said.
    // ponytail: matched by kind (the words before the first number), so a
    // row that grew longer still stays quiet on length
    if let Some(old) = row
        .supersedes
        .as_deref()
        .and_then(|s| log.rows.iter().find(|r| r.id == s))
    {
        let before: Vec<String> = shape_warnings(old, cfg)
            .iter()
            .map(|x| kind_of(x))
            .collect();
        shape.retain(|x| !before.contains(&kind_of(x)));
    }
    w.extend(shape);
    w
}

/// A warning's kind: its words before the first digit (`text is 1203 chars`
/// and `text is 900 chars` are one kind).
fn kind_of(w: &str) -> String {
    w.split(|c: char| c.is_ascii_digit())
        .next()
        .unwrap_or(w)
        .to_string()
}

/// The warnings about a row's own shape — fat reasons, a long untitled
/// text, a long title — the ones a re-file inherits from the row it replaces.
fn shape_warnings(row: &Row, cfg: &Config) -> Vec<String> {
    let mut w: Vec<String> = fat_reasons(row, cfg)
        .into_iter()
        .map(|r| format!("warning: {r}"))
        .collect();
    // lists show the title, bodies are pulled by id — a long untitled row
    // costs its full text on every push. Thai and CJK have no spaces between
    // words, so chars count too.
    let words = row.text.split_whitespace().count();
    let chars = row.text.chars().count();
    if (words > 60 || chars > 400) && row.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
        w.push(format!(
            "warning: text is {words} words with no title — add --title \"<≤15-word headline>\" so lists stay skimmable"
        ));
    }
    if let Some(t) = row.title.as_deref() {
        let n = t.split_whitespace().count();
        if n > 15 {
            w.push(format!(
                "warning: title is {n} words (aim ≤ 15) — lists show it in full on every push"
            ));
        }
    }
    w
}

/// Levenshtein distance over chars, std only — shared with the CLI's
/// did-you-mean (same short inputs, never many).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push(
                (prev[j] + usize::from(ca != *cb))
                    .min(prev[j + 1] + 1)
                    .min(cur[j] + 1),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}
