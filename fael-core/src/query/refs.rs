//! Id-reference contract (PLAN-fael-id-refs §2): an id is real iff a canonical
//! row exists for it. `Ref` is the union-scope existence answer; `id_tokens`
//! lists the id-shaped tokens prose cites. Pure — no git spawn (core never
//! spawns); branch escalation lives on the binary side (`fael/src/refs.rs`).

use crate::{Log, Row, looks_like_id};
use std::collections::HashMap;

/// Union-scope existence of one token: exact id or case-insensitive prefix
/// over rows AND closes (a close row's own id counts — it exists, only the
/// row it points at is gone). `Many` is ambiguity, never missing: an
/// abbreviation that decayed as the log grew.
///
/// Canonical logic lives in `super::lookup::resolve_identity` (plan
/// fael-secretary c2, F-IDENT); this is the same function under the
/// historical name, so existing callers keep working while CLI/MCP move to
/// the canonical entry point.
pub enum Ref<'a> {
    One(&'a Row),
    Many(Vec<&'a Row>),
    Missing,
}

/// Existence by exact id or case-insensitive prefix. Content rows decide
/// first: a close row, or a carrier (a bump event filed right after its
/// row), must never turn a row's unique prefix into `Many` — that would make
/// `find` reject an id it just printed. Only when no content row matches do
/// the carriers answer, then the closes, so their own ids still exist
/// (`One`/`Many`), never `Missing`. Empty tokens never match.
pub fn ref_state<'a>(log: &'a Log, tok: &str) -> Ref<'a> {
    super::lookup::resolve_identity(log, tok)
}

/// Id-shaped tokens in prose, deduped (case-insensitively), in order. Split
/// on whitespace and trim non-alphanumerics at both ends — the same cut
/// `selfheal::text_targets` uses, so both agree on what a token is.
pub fn id_tokens(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = vec![];
    for tok in text.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric());
        if t.is_empty() || !looks_like_id(t) {
            continue;
        }
        if !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t);
        }
    }
    out
}

/// Id-shaped tokens in `text` with no row behind them (union scope only):
/// `Missing` via the canonical `super::lookup::resolve_identity` — `One`/`Many`
/// resolve, so real and ambiguous citations never report. Pure, no spawn; the
/// binary re-checks candidates against unmerged branches once before reporting.
pub fn phantom_refs(log: &Log, text: &str) -> Vec<String> {
    id_tokens(text)
        .into_iter()
        .filter(|tok| matches!(super::lookup::resolve_identity(log, tok), Ref::Missing))
        .map(String::from)
        .collect()
}

/// `superseded()` with the edge kept: superseded id → the id that replaced it,
/// so a reader can point the old row at its successor. Two edges on one target
/// keep the newer.
pub fn successors(log: &Log) -> HashMap<&str, &str> {
    let rev = super::reverted(log);
    let mut out = HashMap::new();
    for r in &log.rows {
        if let Some(t) = r.supersedes.as_deref()
            && !rev.contains(r.id.as_str())
        {
            out.insert(t, r.id.as_str());
        }
    }
    out
}

/// Rows that continue another without replacing it (`follows`), by the id they
/// follow, oldest first. Read by `find` ("followed by") and nothing else: a
/// follow never hides, closes or revives a row.
pub fn followers(log: &Log) -> HashMap<&str, Vec<&str>> {
    let mut out: HashMap<&str, Vec<&str>> = HashMap::new();
    for r in &log.rows {
        if let Some(t) = r.follows.as_deref() {
            out.entry(t).or_default().push(r.id.as_str());
        }
    }
    out
}
