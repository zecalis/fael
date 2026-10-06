//! What an empty `find` says, and when a short list shows its bodies — both
//! read the same log and filter the call already had, so no extra round and
//! nothing a clock or a model could change.

use super::{Filter, est_tokens, select::find};
use crate::{Log, Row};

/// A list this short, from the first page with no `limit`, shows bodies
/// without `--full`: the next call would be `find <id>` for each row anyway,
/// and two bodies cost what that round costs.
pub const EXPAND_MAX: usize = 2;

/// These `rows` (the whole first page) came back for `f`: show bodies, not
/// titles? Only when every body fits `budget` — a fat row stays a title, since
/// a list must never cost more than the `find <id>` it saves. Never for the
/// session brief (no narrowing at all): that list is a map, not an answer.
pub fn expands(rows: &[&Row], total: usize, f: &Filter, budget: usize) -> bool {
    let bodies: usize = rows.iter().map(|r| est_tokens(&r.text)).sum();
    (1..=EXPAND_MAX).contains(&total)
        && f.offset == 0
        && f.limit.is_none()
        && !(f.is_empty() && !f.all)
        && bodies <= budget
}

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
/// that does not exist — shows next to the ones that did. `files_flag` is how
/// this surface spells `--files`, for the path hint.
pub fn why_empty(log: &Log, f: &Filter, files_flag: &str) -> String {
    let alone = |g: Filter| find(log, &Filter { all: f.all, ..g }).len();
    let mut parts: Vec<String> = vec![];
    let words: Vec<&str> = f.text.as_deref().unwrap_or("").split_whitespace().collect();
    for w in &words {
        let n = alone(Filter {
            text: Some(w.to_string()),
            ..Filter::default()
        });
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
        [one] => {
            let what = one.rsplit_once(" ×").map_or(one.as_str(), |(p, _)| p);
            let hidden = if f.all {
                ""
            } else {
                " (open rows only; --all adds closed)"
            };
            format!("no rows match {what}{hidden}")
        }
        _ => format!("no rows match — each alone: {}", parts.join(" · ")),
    };
    // a path typed as a word searches row text, not the files rows sit on
    if f.files.is_empty() && words.iter().any(|w| path_like(w)) {
        s.push_str(&format!(" · a path? use {files_flag}"));
    }
    s
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
