//! What an empty `find` says, and when a short list shows its bodies — both
//! read the same log and filter the call already had, so no extra round and
//! nothing a clock or a model could change.

use super::{Filter, est_tokens, matching::all_words, select::find};
use crate::{Log, Row, looks_like_id};

/// `--json` is the machine shape and is never cut — truncating it would hand a
/// script fewer rows than it asked for without a word. A caller that did not
/// name a `limit` and got more than the find budget is told once instead, so an
/// agent that reached for `--json` to read ids sees what it just paid.
pub fn json_note(rows: &[&Row], budget: usize) -> Option<String> {
    let t: usize = rows.iter().map(|r| est_tokens(&r.to_line())).sum();
    (t > budget).then(|| {
        format!(
            "{} rows, ~{t} tokens of JSON, over the {budget}-token find budget — add --limit N, or drop --json for the list",
            rows.len()
        )
    })
}

/// The reason a find came back empty: every part of the call counted on its
/// own, so the one that matched nothing — a word the rows never use, a key
/// that does not exist — shows next to the ones that did. The one exception
/// is a text-only search none of whose words any row uses: no breakdown
/// could help (every count is zero), so say so honestly and point at code.
/// `files_flag` is how this surface spells `--files`, for the path hint.
pub fn why_empty(log: &Log, f: &Filter, files_flag: &str) -> String {
    let alone = |g: Filter| find(log, &Filter { all: f.all, ..g }).len();
    let words: Vec<&str> = f.text.as_deref().unwrap_or("").split_whitespace().collect();
    let word_ns: Vec<usize> = words
        .iter()
        .map(|w| {
            alone(Filter {
                text: Some(w.to_string()),
                ..Filter::default()
            })
        })
        .collect();
    if let Some(s) = honest_no_mention(f, &words, &word_ns, files_flag) {
        return with_english(f, s);
    }
    let mut parts: Vec<String> = vec![];
    for (w, n) in words.iter().zip(word_ns.iter()) {
        parts.push(format!("{w:?} ×{n}"));
    }
    if !f.files.is_empty() {
        let n = alone(Filter {
            files: f.files.clone(),
            ..Filter::default()
        });
        parts.push(format!("files ×{n}"));
    }
    let d = Filter::default;
    let singles = [
        (
            "key",
            &f.key,
            Filter {
                key: f.key.clone(),
                ..d()
            },
        ),
        (
            "kind",
            &f.kind,
            Filter {
                kind: f.kind.clone(),
                ..d()
            },
        ),
        (
            "since",
            &f.since,
            Filter {
                since: f.since.clone(),
                ..d()
            },
        ),
        (
            "by",
            &f.by,
            Filter {
                by: f.by.clone(),
                ..d()
            },
        ),
        (
            "to",
            &f.to,
            Filter {
                to: f.to.clone(),
                ..d()
            },
        ),
    ];
    for (label, v, g) in singles {
        if let Some(v) = v {
            parts.push(format!("{label}={v} ×{}", alone(g)));
        }
    }
    if parts.is_empty() {
        return "no rows match".into();
    }
    let mut s = match &parts[..] {
        // one filter: "each alone" adds nothing — say what matched nothing,
        // and that closed rows are out of sight unless --all
        [one] => single_empty(log, f, one),
        _ => format!("no rows match — each alone: {}", parts.join(" · ")),
    };
    // a path typed as a word searches row text, not the files rows sit on
    if f.files.is_empty() && words.iter().any(|w| path_like(w)) {
        s.push_str(&format!(" · a path? use {files_flag}"));
    }
    // a key typed as text (`plan:x`, `#k`, `key:k`, `area:*`) searches row
    // words, never the key field — glob and prefix forms miss even open rows,
    // so name the `--key` to repeat with
    if f.key.is_none()
        && let Some(k) = f.text.as_deref().and_then(key_text)
    {
        s.push_str(&format!(" · a key? use --key {k}"));
    }
    with_english(f, s)
}

/// A non-ASCII text query on an English ledger: one line saying rows are
/// English, so the agent retries in English instead of retrying the same
/// words. Pure text check — no language detection, just ASCII.
fn with_english(f: &Filter, mut s: String) -> String {
    let foreign = f.text.as_deref().is_some_and(|t| !t.is_ascii());
    if foreign {
        s.push_str("\nrows are written in English");
    }
    s
}

/// A miss whose text is one token equal to a filed file's stem: the rows on
/// that file, under `rows on <path>:`. Deterministic file-stem match, no
/// text ranking — the agent typed a file name, not words. `None` for
/// anything else (multi-word, other filters narrowing by file/key, paging,
/// or id-shaped tokens, which stay typed misses per plan fael-id-refs).
pub fn file_stem_fallback<'a>(log: &'a Log, f: &Filter) -> Option<(String, Vec<&'a Row>)> {
    if f.offset > 0 || !f.files.is_empty() || f.key.is_some() {
        return None;
    }
    let q = f.text.as_deref().map(str::trim).filter(|t| !t.is_empty())?;
    if q.split_whitespace().count() != 1 || looks_like_id(q) {
        return None;
    }
    let want = q.to_lowercase();
    let mut files: Vec<&str> = vec![];
    for r in &log.rows {
        for p in &r.files {
            if stem(p).to_lowercase() == want && !files.contains(&p.as_str()) {
                files.push(p.as_str());
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    files.sort_unstable();
    let hits = find(
        log,
        &Filter {
            text: None,
            files: files.iter().map(|s| s.to_string()).collect(),
            key: None,
            kind: f.kind.clone(),
            since: f.since.clone(),
            by: f.by.clone(),
            to: f.to.clone(),
            revisit: f.revisit.clone(),
            recall: None,
            all: f.all,
            limit: None,
            offset: 0,
        },
    );
    if hits.is_empty() {
        return None;
    }
    Some((format!("rows on {}:", files.join(", ")), hits))
}

/// A filed path's stem: basename without its extension (`a/b/x.rs` →
/// `x`). No extension (or an empty stem, like `.gitignore`) keeps the
/// basename, so every path still has a stem to match against.
fn stem(path: &str) -> &str {
    let base = path.rsplit('/').next().unwrap_or(path);
    match base.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => base,
    }
}

/// A text-only search none of whose words any row uses: the cross-product
/// (`each alone … ×0`) reads as if the ledger failed, when the term likely
/// lives in code. `None` for every other empty shape, which keeps its
/// per-part breakdown.
fn honest_no_mention(
    f: &Filter,
    words: &[&str],
    word_ns: &[usize],
    files_flag: &str,
) -> Option<String> {
    if words.is_empty()
        || !word_ns.iter().all(|&n| n == 0)
        || !f.files.is_empty()
        || f.key.is_some()
        || f.kind.is_some()
        || f.since.is_some()
        || f.by.is_some()
        || f.to.is_some()
        || f.revisit.is_some()
        || f.recall.is_some()
    {
        return None;
    }
    let q = words.join(" ");
    let mut s = format!(
        "no row mentions {q:?} — if it is a code symbol, it lives in code: search with rg or file it with fael add"
    );
    if words.iter().any(|w| path_like(w)) {
        s.push_str(&format!(" · a path? use {files_flag}"));
    }
    if let Some(k) = f.text.as_deref().and_then(key_text) {
        s.push_str(&format!(" · a key? use --key {k}"));
    }
    Some(s)
}

/// One filter: "each alone" adds nothing — say what matched nothing, and that
/// closed rows are out of sight unless --all.
fn single_empty(log: &Log, f: &Filter, one: &str) -> String {
    let what = one.rsplit_once(" ×").map_or(one, |(p, _)| p);
    let (gone, spent) = (
        log.dormant.len().saturating_sub(log.retired.len()),
        log.retired.len(),
    );
    let hidden = match (f.all, gone, spent) {
        (true, ..) => String::new(),
        (false, 0, 0) => " (open rows only; --all adds closed)".into(),
        (false, n, 0) => format!(" (open rows only; --all adds closed and {n} dormant)"),
        (false, 0, m) => format!(" (open rows only; --all adds closed and {m} retired)"),
        (false, n, m) => {
            format!(" (open rows only; --all adds closed, {n} dormant and {m} retired)")
        }
    };
    format!("no rows match {what}{hidden}")
}

/// Most rows a `nearest` answer lists: best guesses, not a page.
const NEAREST_MAX: usize = 5;

/// The exact `--all` answer when the open-only call came back empty: the same
/// filter with closed rows in, every match, unpaged. `None` when the caller
/// already asked for all, past the first page, or nothing matches even with
/// closed rows in. The label says no open row matched; `render` marks each
/// row closed, dormant or retired.
pub fn closed_fallback<'a>(log: &'a Log, f: &Filter) -> Option<(String, Vec<&'a Row>)> {
    if f.all || f.offset > 0 {
        return None;
    }
    let closed = find(
        log,
        &Filter {
            all: true,
            ..f.clone()
        },
    );
    if closed.is_empty() {
        return None;
    }
    let asleep = closed
        .iter()
        .filter(|r| log.dormant.contains(&r.id) && !log.retired.contains(&r.id))
        .count();
    let spent = closed.iter().any(|r| log.retired.contains(&r.id));
    let label = match asleep {
        0 if spent => format!(
            "no open row matches — {} closed, superseded or retired:",
            closed.len()
        ),
        0 => format!(
            "no open row matches — {} closed or superseded:",
            closed.len()
        ),
        n => format!(
            "no open row matches — {} closed, superseded or dormant ({n} dormant: its files changed since, a bare `fael bump <id>` revives one):",
            closed.len()
        ),
    };
    Some((label, closed))
}

/// A find that came back empty, answered in the same call — the agent's next
/// find would be its own guess at a wider one (the repeat `friction` counts).
/// First the same call with closed rows in; else, for two or more words, the
/// rows matching the most of them (at least half), every other filter kept. `None` when
/// neither finds a row, or past the first page. The label says which it is.
pub fn nearest<'a>(log: &'a Log, f: &Filter) -> Option<(String, Vec<&'a Row>)> {
    if f.offset > 0 {
        return None;
    }
    let top = |rows: Vec<&'a Row>| rows.into_iter().take(NEAREST_MAX).collect();
    if let Some((label, closed)) = closed_fallback(log, f) {
        return Some((label, top(closed)));
    }
    // a key typed as text (`plan:x`, `#k`, `key:k`, `area:*`): the words never
    // match a glob or a `key:`/`#` prefix, but `--key` does — retry it before
    // the partial-word guesses, other filters kept
    if f.key.is_none()
        && let Some(k) = f.text.as_deref().and_then(key_text)
    {
        let keyed = find(
            log,
            &Filter {
                text: None,
                key: Some(k.clone()),
                ..f.clone()
            },
        );
        if !keyed.is_empty() {
            return Some((
                format!("no rows match text — {} with key {k}:", keyed.len()),
                top(keyed),
            ));
        }
        if !f.all {
            let closed = find(
                log,
                &Filter {
                    text: None,
                    key: Some(k.clone()),
                    all: true,
                    ..f.clone()
                },
            );
            if !closed.is_empty() {
                return Some((
                    format!(
                        "no open row matches text — {} hidden with key {k}:",
                        closed.len()
                    ),
                    top(closed),
                ));
            }
        }
    }
    let q = f.text.as_deref()?.to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let rest = find(
        log,
        &Filter {
            text: None,
            ..f.clone()
        },
    );
    let hits = |r: &Row| words.iter().filter(|w| all_words(w, r)).count();
    // at least half the words: one common word of four (`agent`) is no answer
    let best = rest
        .iter()
        .map(|r| hits(r))
        .max()
        .filter(|&n| n > 0 && n * 2 >= words.len())?;
    let rows: Vec<&Row> = rest.into_iter().filter(|r| hits(r) == best).collect();
    let label = format!(
        "no row has every word — {} with {best} of {}:",
        rows.len(),
        words.len()
    );
    Some((label, top(rows)))
}

/// `a/b`, or `name.ext` with a letters-only extension (`v0.17.1` is not one).
fn path_like(w: &str) -> bool {
    w.contains('/')
        || w.rsplit_once('.').is_some_and(|(stem, ext)| {
            !stem.is_empty()
                && (1..=4).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphabetic())
        })
}

/// The `--key` a key-shaped text retries as: one token with a `:` that is no
/// path (`a/b:c`), after one `#` or `key:` prefix — `plan:x`, `#k`, `key:k`,
/// `area:*`. `None` for prose (`two words`, `trailing:`) and paths.
fn key_text(text: &str) -> Option<String> {
    if text.split_whitespace().count() != 1 {
        return None;
    }
    let t = text.strip_prefix('#').unwrap_or(text);
    let t = t.strip_prefix("key:").unwrap_or(t);
    let (a, b) = t.split_once(':')?;
    (!a.is_empty() && !b.is_empty() && !t.contains('/')).then(|| t.to_string())
}
