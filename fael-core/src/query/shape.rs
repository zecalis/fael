//! The shape faults `add` rejects before writing: the ones the caller fixes
//! in the same call. `lookup`'s warnings reuse the topic-list and untitled
//! checks, so the warning and the reject never drift apart.

use crate::{Config, Log, Row};

/// Chars per clause below which `;`-separated chunks read as a topic list
/// rather than prose sentences: three-plus separators warn only while the
/// row stays short (`chars < seps * CHARS_PER_CLAUSE`). Calibrated against
/// real ledger rows (604–949 chars, 3–5 seps, all single-topic, all silent).
/// Char-based so Thai/CJK (no spaces) judge by the same ruler as English.
const CHARS_PER_CLAUSE: usize = 100;

/// `·`/`;` joining topics. `·` joins topics — two of them is a list of
/// topics. `;` is also plain English clause punctuation inside one topic:
/// three separators read as a topic list only while the clauses stay short —
/// prose clauses run a sentence long, so the check is a density and a long
/// row needs more `;` per char before it stops being prose.
pub(super) fn topic_list(row: &Row) -> Option<String> {
    let chars = row.text.chars().count();
    let mid = row.text.chars().filter(|&c| c == '·').count();
    let seps = mid + row.text.chars().filter(|&c| c == ';').count();
    (mid >= 2 || (seps >= 3 && chars < seps * CHARS_PER_CLAUSE)).then(|| {
        format!(
            "text has {seps} topic separators (; / ·) — one topic per row: split it, \
each with --key area:topic, so one can be superseded alone"
        )
    })
}

/// A long text with no title. Lists show the title, bodies are pulled by id —
/// a long untitled row costs its full text on every push. Thai and CJK have
/// no spaces between words, so chars count too.
pub(super) fn untitled(row: &Row) -> Option<String> {
    let words = row.text.split_whitespace().count();
    let chars = row.text.chars().count();
    ((words > 60 || chars > 400) && row.title.as_deref().is_none_or(|t| t.trim().is_empty()))
        .then(|| {
            let n = if words > 60 { format!("{words} words") } else { format!("{chars} chars") };
            format!(
                "text is {n} with no title — add --title \"<≤15-word headline>\" so lists stay skimmable"
            )
        })
}

/// The shape faults `add` rejects before writing (unless `--force`): a topic
/// list and a long untitled text — the two the caller fixes in the same call
/// (split it, add --title). A warning after the write came too late: the row
/// was already filed and nobody went back. Length, a missing key and a
/// docs-only list stay warnings. What `old` (the row this one re-files) already
/// had is not new, so it never blocks: `--supersedes <id> --replace` on a
/// fat row must still go through.
pub fn shape_rejects(row: &Row, old: Option<&Row>) -> Vec<String> {
    let mut r = vec![];
    if topic_list(row).is_some() && old.is_none_or(|o| topic_list(o).is_none()) {
        r.extend(topic_list(row));
    }
    if untitled(row).is_some() && old.is_none_or(|o| untitled(o).is_none()) {
        r.extend(untitled(row));
    }
    r
}

/// `shape_rejects` as the add path's reject: nothing is written, and the
/// message names every other fat reason too, so one re-run fixes them all.
/// `old` is the id the row re-files, if any.
pub fn add_gate(row: &Row, log: &Log, cfg: &Config, old: Option<&str>) -> Result<(), String> {
    let old = old.and_then(|s| super::resolve_row(log, s).ok());
    let mut why = shape_rejects(row, old);
    if why.is_empty() {
        return Ok(());
    }
    for f in super::fat_reasons(row, cfg) {
        if !why.contains(&f) {
            why.push(f);
        }
    }
    Err(format!(
        "rejected: nothing written — {} (or add --force to file it as is)",
        why.join(" · ")
    ))
}
