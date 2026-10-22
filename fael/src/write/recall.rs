//! Closed-issue recall (PLAN-fael-context-loop chunk 5): a new issue on a file
//! that already carries a closed one says so, with the command to link them.
//! Only the agent can say it is the same bug — fael names the candidates (at
//! most two, newest first) and never links them itself. Silent without one.

use crate::{core, hook};

const MAX: usize = 2;

/// Title-word overlap that makes two open issues on a shared file "maybe the
/// same bug". ponytail: bag-of-words Jaccard — misses paraphrases and
/// synonyms; upgrade to stemming/embeddings only if stats show missed dups.
const NEAR: f64 = 0.5;

fn norm(t: &str) -> String {
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn words(t: &str) -> std::collections::HashSet<String> {
    t.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// An open row with the same kind, key, normalized text and `--files` set as `row`
/// — filing it again would only add noise (idempotency, not a judgement).
pub(crate) fn exact<'a>(log: &'a core::Log, row: &core::Row) -> Option<&'a core::Row> {
    let (text, files) = (
        norm(&row.text),
        row.files.iter().collect::<std::collections::BTreeSet<_>>(),
    );
    let gone = core::closed(log);
    let linked = core::superseded(log);
    log.rows.iter().rev().find(|r| {
        r.kind == row.kind
            && r.key == row.key // a re-key is a different row, not a repeat
            && !gone.contains(r.id.as_str())
            && !linked.contains(r.id.as_str())
            && norm(&r.text) == text
            && r.files.iter().collect::<std::collections::BTreeSet<_>>() == files
    })
}

/// `already open: <id> "<title>" — not filed again`.
pub(crate) fn already_line(log: &core::Log, old: &core::Row) -> String {
    format!(
        "already open: {} \"{}\" — not filed again",
        core::abbrev(log).short(&old.id),
        old.display_title()
    )
}

/// Info lines for open issues on a shared file whose title reads like `row`'s
/// (at most two, newest first). `log` is the log before the row.
pub(crate) fn near(log: &core::Log, row: &core::Row) -> Vec<String> {
    if row.kind != "issue" || row.supersedes.is_some() {
        return vec![];
    }
    let mine = words(&row.display_title());
    let (gone, linked) = (core::closed(log), core::superseded(log));
    let mut hits: Vec<&core::Row> = log
        .rows
        .iter()
        .filter(|r| r.kind == "issue")
        .filter(|r| !gone.contains(r.id.as_str()) && !linked.contains(r.id.as_str()))
        .filter(|r| shares_file(r, row))
        .filter(|r| {
            let theirs = words(&r.display_title());
            let both = mine.intersection(&theirs).count() as f64;
            let all = mine.union(&theirs).count() as f64;
            all > 0.0 && both / all >= NEAR
        })
        .collect();
    hits.sort_by(|a, b| b.id.cmp(&a.id));
    let w = core::abbrev(log).with(&row.id);
    let new = w.short(&row.id);
    hits.iter()
        .take(MAX)
        .map(|c| {
            let old = w.short(&c.id);
            format!(
                "open on these files: {old} \"{}\" — same bug? fael close {new} \"dup of {old}\"",
                c.display_title()
            )
        })
        .collect()
}

fn shares_file(a: &core::Row, b: &core::Row) -> bool {
    a.files
        .iter()
        .any(|f| b.files.contains(f) && !hook::is_anchor(f) && !core::is_glob(f))
}

/// Info lines for the issue just filed (`log` is the log before it). None when
/// the row is no issue, already supersedes something (the caller linked it, or
/// self-heal did), or no closed issue shares a file with it.
pub(crate) fn lines(log: &core::Log, row: &core::Row) -> Vec<String> {
    if row.kind != "issue" || row.supersedes.is_some() {
        return vec![];
    }
    let gone = core::closed(log);
    let linked = core::superseded(log);
    let mut hits: Vec<&core::Row> = log
        .rows
        .iter()
        .filter(|r| r.kind == "issue" && gone.contains(r.id.as_str()))
        .filter(|r| !linked.contains(r.id.as_str()))
        .filter(|r| shares_file(r, row))
        .collect();
    hits.sort_by(|a, b| b.id.cmp(&a.id)); // ids are time-ordered
    let w = core::abbrev(log).with(&row.id);
    let new = w.short(&row.id);
    hits.iter()
        .take(MAX)
        .map(|c| {
            let old = w.short(&c.id);
            format!(
                "closed earlier on these files (left closed): {old} \"{}\" — only if it is \
the same bug: fael add issue \"<text>\" --supersedes {old}, then fael close {new} \"refiled\"",
                c.display_title()
            )
        })
        .collect()
}
