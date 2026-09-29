//! Decision: the Explicit > Identity > Heuristic policy table over Evidence
//! (chunk 2 of PLAN-fael-selfheal-verdict).
//!
//! Chunk 1 read Evidence through the legacy order; this chunk declares the
//! order instead. Every open row is classified, per class, as Eligible (the
//! class may act on it), Ineligible (seen, never acted on — lower classes may
//! still decide) or Blocks (the class found its identity but cannot act, so
//! no lower class may pick something else instead). The first class in table
//! order with an eligible candidate decides — one acts, several hold — and a
//! class with only Blocks reports "kept open". The Verdict and the renderer
//! are unchanged: the table decides the same outcomes the order did, byte
//! for byte.

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

pub(crate) fn decide(
    log: &core::Log,
    open: &[&core::Row],
    st: &core::Stamp,
    row: &core::Row,
    flag: Option<&str>,
) -> Verdict {
    // A caller-given flag is Explicit too: it resolves in core, so the text
    // never overrides it — a broken one is rescued by the text alone.
    if flag.is_some() {
        let cands = observe(log, open, st, row);
        return decide_flag(log, &cands, flag);
    }
    let cands = observe(log, open, st, row);
    // The policy table, highest class first: the first class that claims any
    // candidate decides. A class that found its identity but cannot act
    // reports "kept open" instead of yielding to a lower class (Blocks) —
    // that is why a caller-given key is never overridden by a files guess.
    if let Some(v) = explicit(&cands, row) {
        return v;
    }
    if let Some(v) = identity(&cands, st, row) {
        return v;
    }
    if let Some(v) = heuristic(&cands, row) {
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

/// What one class says about one candidate: eligible to act, seen but not
/// actionable (lower classes may still decide), or an identity that blocks
/// every lower class from picking instead — still reaching the renderer as
/// "kept open", never silently dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    Eligible,
    Ineligible,
    Blocks(&'static str),
}

/// Explicit: the text names a row after a "supersede*" word. A resolving
/// `--supersedes` flag never reaches here (see `decide`): the flag the caller
/// typed is intent as explicit as the text, so the text cannot override it.
/// A merely mentioned id is Ineligible — observed, never acted on.
fn explicit_standing(c: &Candidate) -> Option<Standing> {
    match c.evidence.named {
        NameRel::AfterSupersede => Some(Standing::Eligible),
        NameRel::Mentioned => Some(Standing::Ineligible),
        NameRel::No => None,
    }
}

/// Identity: the caller's key plus the row's kind, the topic's identity. A
/// same-key row of another writer, or an issue that is a different finding,
/// Blocks: the caller said what this is about, so no heuristic may pick a
/// substitute. Auto-key (e) never reaches here: it runs after `heal`, so a
/// key the caller did not write can't close a row.
fn identity_standing(c: &Candidate, st: &core::Stamp, row: &core::Row) -> Option<Standing> {
    if row.key.is_none() || c.evidence.key != KeyRel::Same || c.evidence.kind != Rel::Same {
        return None;
    }
    if row.kind == "issue" && !same_finding(&c.evidence) {
        return Some(Standing::Blocks("a different finding"));
    }
    if c.row.by != st.by {
        return Some(Standing::Blocks("another writer"));
    }
    Some(Standing::Eligible)
}

/// Heuristic: an open note of mine on this branch overlapping these files.
/// No Blocks here — a guess never outranks an identity, it only fires when
/// neither higher class claimed anything.
fn heuristic_standing(c: &Candidate) -> Option<Standing> {
    (c.evidence.kind == Rel::Same
        && c.evidence.writer == Rel::Same
        && c.evidence.branch == Rel::Same
        && c.evidence.files.shared > 0)
        .then_some(Standing::Eligible)
}

/// Explicit resolves first: exactly one named row acts (notes also list what
/// else overlaps, like Identity does); several hold and name them; mentions
/// alone claim nothing, so lower classes may still decide.
fn explicit(cands: &[Candidate], row: &core::Row) -> Option<Verdict> {
    let named: Vec<&Candidate> = cands
        .iter()
        .filter(|c| explicit_standing(c) == Some(Standing::Eligible))
        .collect();
    match named.as_slice() {
        [one] => Some(Verdict::TextAct {
            target: one.row.id.clone(),
            also: also_if_note(cands, row, &one.row.id),
        }),
        many @ [_, ..] => Some(Verdict::TextHold { targets: ids(many) }),
        [] => None,
    }
}

/// Identity resolves second. The class claims every same-kind same-key row;
/// one of my own acts, several hold, and a claim the class cannot act on
/// Blocks: another writer's row, or a different finding, is kept and named —
/// never silently yielded to the files guess below. `None` — no key on the
/// row, or no open row carrying that kind + key — falls through.
fn identity(cands: &[Candidate], st: &core::Stamp, row: &core::Row) -> Option<Verdict> {
    row.key.as_ref()?;
    let claimed: Vec<&Candidate> = cands
        .iter()
        .filter(|c| identity_standing(c, st, row).is_some())
        .collect();
    if claimed.is_empty() {
        return None;
    }
    // An `issue` is a finding, not a topic — a key may hold several — so an
    // issue candidate must also be the same finding; a distinct issue sharing
    // the key is kept and named, never swallowed.
    let hits: Vec<&Candidate> = claimed
        .iter()
        .copied()
        .filter(|c| !finding_blocked(c, st, row))
        .collect();
    if hits.is_empty() {
        // only reachable for an issue: every claim is a different finding,
        // so name what stays open and supersede nothing
        return Some(Verdict::KeyIssueKept {
            targets: ids(&claimed),
        });
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

/// A claim blocked as a different finding — the only claims `identity`
/// sets aside before acting or holding.
fn finding_blocked(c: &Candidate, st: &core::Stamp, row: &core::Row) -> bool {
    matches!(
        identity_standing(c, st, row),
        Some(Standing::Blocks("a different finding"))
    )
}

/// Heuristic resolves last, notes only: one overlapping note acts, several
/// hold. It fires only when neither higher class claimed anything — a guess
/// never outranks an identity.
fn heuristic(cands: &[Candidate], row: &core::Row) -> Option<Verdict> {
    if row.kind != "note" {
        return None;
    }
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
        .filter(|c| explicit_standing(c) == Some(Standing::Eligible))
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
/// Evidence instead of re-asking the rows: exactly the Heuristic class.
fn files_match<'b>(cands: &'b [Candidate]) -> Vec<&'b Candidate<'b>> {
    cands
        .iter()
        .filter(|c| heuristic_standing(c) == Some(Standing::Eligible))
        .collect()
}

fn ids(cands: &[&Candidate]) -> Vec<String> {
    cands.iter().map(|c| c.row.id.clone()).collect()
}
