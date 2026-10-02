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

use super::evidence::{
    Candidate, Evidence, KeyRel, NameRel, Rel, observe, open_rows, same_finding,
};
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
    /// (b) the one open note of mine on this branch overlapping these files,
    /// and neither it nor the new row has a key — no topic to lose.
    FilesAct { target: String },
    /// (b) several overlap, or one with a key on either side: file the row,
    /// name them all — shared files are related, not proof of a replacement.
    FilesMany { targets: Vec<String> },
    /// (chunk 3) a cross-key act: the acted row's key differs from the new
    /// row's — only an explicit act (the text names the row) reaches it, the
    /// files guess never acts across keys. The inner act stands, but the key is
    /// the weakest evidence, so `[selfheal] cross_key` picks the exposure:
    /// Warn prints the act as one `warning:` line (an ask), Info as info,
    /// Off silently. Only `Differ` wraps: `OnlyNew`, `OnlyOld` and `Neither`
    /// carry no conflict to expose.
    CrossKey {
        inner: Box<Verdict>,
        old: String,
        new: String,
    },
    /// Nothing matched: file the row, say nothing.
    Noop,
}

/// What `evaluate` settled on: the Verdict, the Heal it renders to, and the
/// acted row's evidence — one shared source for the write path,
/// `add --dry-run` and MCP `dry_run`, so the three can never disagree.
pub(crate) struct Evaluated {
    pub verdict: Verdict,
    pub heal: Heal,
    pub evidence: Option<Evidence>,
}

/// The single Verdict source (chunk 2 of PLAN-fael-selfheal-restore): settle
/// the Verdict without writing anything. Order is Explicit > Identity >
/// Heuristic (see `decide`): (d) the text naming a row, then (c) same kind +
/// key, then (b) notes on the same writer + branch with overlapping files.
/// The write path acts on exactly this.
pub(crate) fn evaluate(
    log: &core::Log,
    st: &core::Stamp,
    row: &core::Row,
    flag: Option<&str>,
    cross: core::CrossKey,
) -> Evaluated {
    let open = open_rows(log);
    // ids print at their shortest unique prefix, same as render/doctor, so any
    // of them pastes straight into `--supersedes` — unique against the row
    // being added too, which a fast caller may write in the same millisecond
    let w = core::abbrev(log).with(&row.id);
    let verdict = decide(log, &open, st, row, flag);
    let source = source_of(&verdict);
    let mut h = render(&verdict, row, st, &w, flag, cross);
    h.source = source;
    name_the_replaced(log, &w, &mut h);
    // observe runs a second time here (decide already did): a scan over open
    // rows is nothing next to the git spawns around it, and sharing one
    // observation would widen every signature between them.
    let evidence = verdict.target().and_then(|t| {
        observe(log, &open, st, row)
            .into_iter()
            .find(|c| c.row.id == t)
            .map(|c| c.evidence)
    });
    Evaluated {
        verdict,
        heal: h,
        evidence,
    }
}

/// A self-heal supersede names what it replaced and how to undo it: a row
/// on the same key can be another topic, and an id alone does not say so.
/// Only fael's own "superseded …" line — a caller's `--supersedes` chose.
fn name_the_replaced(log: &core::Log, w: &core::Abbrev, h: &mut Heal) {
    let (Some(t), Some(first)) = (h.supersedes.as_deref(), h.notes.first_mut()) else {
        return;
    };
    if let Some(old) = log.rows.iter().find(|r| r.id == t)
        && first.starts_with("superseded ")
    {
        first.push_str(&format!(
            " — was \"{}\"; wrong one? fael restore {}",
            old.display_title(),
            w.short(t)
        ));
    }
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
        return wrap_cross(&cands, v);
    }
    if let Some(v) = identity(&cands, st, row) {
        return wrap_cross(&cands, v);
    }
    if let Some(v) = heuristic(&cands, row) {
        return wrap_cross(&cands, v);
    }
    Verdict::Noop
}

/// A caller-given flag: resolves → untouched; resolves to nothing → the text
/// may rescue it with exactly one open row, or the original reject stands
/// (0 or several).
fn decide_flag(log: &core::Log, cands: &[Candidate], flag: Option<&str>) -> Verdict {
    let f = flag.unwrap_or_default();
    if core::resolve(log, f).is_ok() {
        // the caller named it and it resolved — full justification, nothing
        // to expose, so this never wraps below
        return Verdict::FlagPassthrough;
    }
    match named(cands).as_slice() {
        [one] => wrap_cross(
            cands,
            Verdict::FlagRescued {
                target: one.row.id.clone(),
            },
        ),
        _ => Verdict::FlagUnresolved,
    }
}

/// A cross-key act exposes the move: wrap any act whose target's key differs
/// from the new row's, so the renderer — which owns the `[selfheal]
/// cross_key` exposure — sees it. Explicit included: naming the row justifies
/// the act, not the silence about the key moving underneath it.
fn wrap_cross(cands: &[Candidate], v: Verdict) -> Verdict {
    let target = match &v {
        Verdict::TextAct { target, .. }
        | Verdict::KeyAct { target, .. }
        | Verdict::FilesAct { target }
        | Verdict::FlagRescued { target } => target,
        _ => return v,
    };
    let moved = cands
        .iter()
        .find(|c| &c.row.id == target)
        .and_then(|c| match &c.evidence.key {
            KeyRel::Differ { old, new } => Some((old.clone(), new.clone())),
            _ => None,
        });
    match moved {
        Some((old, new)) => Verdict::CrossKey {
            inner: Box::new(v),
            old,
            new,
        },
        None => v,
    }
}

/// Provenance for the row's `supersedes` (chunk 3): which rule filed it, so
/// restore can trace an edge back to its cause. Cross-key acts append
/// `:cross-key` — the exposure (`warning:` vs info vs silent) is the knob's
/// job, the cause is recorded either way. Holds and keeps act on nothing.
fn source_of(v: &Verdict) -> Option<String> {
    match v {
        Verdict::FlagPassthrough => Some("caller:flag".into()),
        Verdict::FlagRescued { .. } | Verdict::TextAct { .. } => Some("explicit:text".into()),
        Verdict::KeyAct { .. } => Some("identity:key".into()),
        Verdict::FilesAct { .. } => Some("heuristic:files".into()),
        Verdict::CrossKey { inner, .. } => source_of(inner).map(|s| format!("{s}:cross-key")),
        _ => None,
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

/// Heuristic: an open note of mine on this branch overlapping these files —
/// related, which is all `heuristic` may act on (it decides what to do with it).
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

/// Heuristic resolves last, notes only. Shared files prove two notes are
/// related, not that one replaces the other, so only the one pair with no
/// topic on either side — both keyless — acts. A key on either side names a
/// topic, and a lone overlap or several are kept and named, never picked: a
/// silent hide costs the next reader a todo, a kept note costs one `close`.
/// It fires only when neither higher class claimed anything.
fn heuristic(cands: &[Candidate], row: &core::Row) -> Option<Verdict> {
    if row.kind != "note" {
        return None;
    }
    match files_match(cands).as_slice() {
        [] => None,
        [one] if one.evidence.key == KeyRel::Neither => Some(Verdict::FilesAct {
            target: one.row.id.clone(),
        }),
        many => Some(Verdict::FilesMany { targets: ids(many) }),
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
