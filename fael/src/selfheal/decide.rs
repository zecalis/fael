//! Decision, temporary: the legacy order (d)→(c)→(b) over Evidence
//! (chunk 1 of PLAN-fael-selfheal-verdict).
//!
//! Chunk 1 changes the shape, not the outcome: the decider reads Evidence
//! instead of re-asking the log, but applies the same order the if-chain used.
//! Chunk 2 replaces `decide` with the declared Explicit > Identity >
//! Heuristic policy table — the Verdict and the renderer stay as they are.

use super::evidence::{Candidate, KeyRel, NameRel, Rel, observe, open_rows, same_finding};
use super::render::{Heal, render};
use crate::core;

/// What self-heal decided — the settled outcome, before any words are
/// printed. The renderer turns this into the byte-identical info lines.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Verdict {
    /// A caller-given flag that resolves: it passes through, no words.
    FlagPassthrough,
    /// A flag that resolves to nothing, rescued by the one row the text names.
    FlagRescued { target: String },
    /// A flag that resolves to nothing and the text names nothing usable —
    /// the original reject stands in core.
    FlagUnresolved,
    /// (d) the text names exactly one open row.
    TextAct { target: String, also: Vec<String> },
    /// (d) the text names several: file the row, name them all.
    TextHold { targets: Vec<String> },
    /// (c) the caller's key is the topic's identity: one of my own rows.
    KeyAct { target: String, also: Vec<String> },
    /// (c) an issue sharing the key is a different finding — kept and named.
    KeyIssueKept { targets: Vec<String> },
    /// (c) the key's row belongs to another writer — kept and named, and
    /// lower rules must not pick something else instead.
    KeyOtherWriter { target: String },
    /// (c) several rows share kind + key: file the row, name them all.
    KeyMany { targets: Vec<String> },
    /// (b) one open note of mine on this branch overlaps these files.
    FilesAct { target: String },
    /// (b) several overlap: file the row, name them all.
    FilesMany { targets: Vec<String> },
    /// Nothing matched: file the row, say nothing.
    Noop,
}

/// Fill an absent `--supersedes` from the open log: (d) the text that names a
/// row, then (c) same kind + key — the key is the identity of the topic, any
/// branch — then (b) notes on the same writer + branch with overlapping files.
/// Zero matches → nothing; exactly one of mine → supersede it and say so;
/// several, or one of another writer → file the row and list what was kept. A
/// caller-given flag passes through untouched unless it resolves to nothing,
/// in which case the text is the only place left to say what to supersede.
pub(crate) fn heal(
    log: &core::Log,
    st: &core::Stamp,
    row: &core::Row,
    flag: Option<&str>,
) -> Result<Heal, String> {
    let open = open_rows(log);
    // ids print at their shortest unique prefix, same as render/doctor, so any
    // of them pastes straight into `--supersedes` — unique against the row
    // being added too, which a fast caller may write in the same millisecond
    let w = core::abbrev(log).with(&row.id);
    let verdict = decide(log, &open, st, row, flag);
    Ok(render(&verdict, row, st, &w, flag))
}

fn decide(
    log: &core::Log,
    open: &[&core::Row],
    st: &core::Stamp,
    row: &core::Row,
    flag: Option<&str>,
) -> Verdict {
    if flag.is_some() {
        let cands = observe(log, open, st, row);
        return decide_flag(log, &cands, flag);
    }
    let cands = observe(log, open, st, row);
    // (d) the text says what this row supersedes but the flag was left off:
    // an id after a "supersede*" word, never an id merely mentioned in passing
    if let Some(v) = decide_text(&cands, row) {
        return v;
    }
    // (c) the caller's key is the strongest identity — see `decide_key`
    if let Some(v) = decide_key(&cands, st, row) {
        return v;
    }
    // (b) files: notes only, same writer + branch, overlapping files
    if row.kind == "note"
        && let Some(v) = decide_files(&cands)
    {
        return v;
    }
    Verdict::Noop
}

/// A caller-given flag: resolves → untouched; resolves to nothing → the text
/// may rescue it with exactly one open row, or the original reject stands
/// (0 or several).
fn decide_flag(log: &core::Log, cands: &[Candidate], flag: Option<&str>) -> Verdict {
    let f = flag.unwrap_or_default();
    if core::resolve(log, f).is_ok() {
        return Verdict::FlagPassthrough;
    }
    match named(cands).as_slice() {
        [one] => Verdict::FlagRescued {
            target: one.row.id.clone(),
        },
        _ => Verdict::FlagUnresolved,
    }
}

/// (d): exactly one named row → act (notes list what else overlaps, like
/// (c) does); several → hold and name them; none → fall through.
fn decide_text(cands: &[Candidate], row: &core::Row) -> Option<Verdict> {
    match named(cands).as_slice() {
        [one] => Some(Verdict::TextAct {
            target: one.row.id.clone(),
            also: also_if_note(cands, row, &one.row.id),
        }),
        many @ [_, ..] => Some(Verdict::TextHold { targets: ids(many) }),
        [] => None,
    }
}

/// (c) The caller's key is the strongest identity: same kind + key, any
/// branch, but only my own row is mine to close. An `issue` is a finding, not
/// a topic — a key may hold several — so an issue candidate must also be the
/// same finding; a distinct issue sharing the key is kept and named, never
/// swallowed. `None` — no key on the row, or no open row carrying that kind +
/// key — falls through to (b). Auto-key (e) never reaches here: it runs after
/// `heal`, so a key the caller did not write can't close a row.
fn decide_key(cands: &[Candidate], st: &core::Stamp, row: &core::Row) -> Option<Verdict> {
    row.key.as_deref()?;
    let all: Vec<&Candidate> = cands
        .iter()
        .filter(|c| c.evidence.key == KeyRel::Same && c.evidence.kind == Rel::Same)
        .collect();
    let hits: Vec<&Candidate> = if row.kind == "issue" {
        all.iter()
            .copied()
            .filter(|c| same_finding(&c.evidence))
            .collect()
    } else {
        all.clone()
    };
    if hits.is_empty() {
        // only reachable for an issue: `all` was non-empty but every row is a
        // different finding, so name what stays open and supersede nothing
        if !all.is_empty() {
            return Some(Verdict::KeyIssueKept { targets: ids(&all) });
        }
        return None;
    }
    match hits.as_slice() {
        [one] if one.row.by == st.by => Some(Verdict::KeyAct {
            target: one.row.id.clone(),
            also: also_if_note(cands, row, &one.row.id),
        }),
        [one] => Some(Verdict::KeyOtherWriter {
            target: one.row.id.clone(),
        }),
        many @ [_, ..] => Some(Verdict::KeyMany { targets: ids(many) }),
        [] => None,
    }
}

/// (b): open notes, same writer + branch, overlapping files.
fn decide_files(cands: &[Candidate]) -> Option<Verdict> {
    match files_match(cands).as_slice() {
        [one] => Some(Verdict::FilesAct {
            target: one.row.id.clone(),
        }),
        many @ [_, ..] => Some(Verdict::FilesMany { targets: ids(many) }),
        [] => None,
    }
}

/// Candidates the text names after a "supersede*" word.
fn named<'b>(cands: &'b [Candidate]) -> Vec<&'b Candidate<'b>> {
    cands
        .iter()
        .filter(|c| c.evidence.named == NameRel::AfterSupersede)
        .collect()
}

/// The open notes besides `target` that overlap these files — named so a note
/// some other rule kept open never hides behind the row that was superseded.
/// Notes only: a decision or issue that acts names no "also".
fn also_if_note(cands: &[Candidate], row: &core::Row, target: &str) -> Vec<String> {
    if row.kind != "note" {
        return vec![];
    }
    files_match(cands)
        .into_iter()
        .filter(|c| c.row.id != target)
        .map(|c| c.row.id.clone())
        .collect()
}

/// Open notes, same writer + branch, overlapping files — chunk 3b, read off
/// Evidence instead of re-asking the rows.
fn files_match<'b>(cands: &'b [Candidate]) -> Vec<&'b Candidate<'b>> {
    cands
        .iter()
        .filter(|c| {
            c.evidence.kind == Rel::Same
                && c.evidence.writer == Rel::Same
                && c.evidence.branch == Rel::Same
                && c.evidence.files.shared > 0
        })
        .collect()
}

fn ids(cands: &[&Candidate]) -> Vec<String> {
    cands.iter().map(|c| c.row.id.clone()).collect()
}
