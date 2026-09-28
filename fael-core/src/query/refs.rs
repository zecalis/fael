//! Id-reference contract (PLAN-fael-id-refs §2): an id is real iff a canonical
//! row exists for it. `Ref` is the union-scope existence answer; `id_tokens`
//! lists the id-shaped tokens prose cites. Pure — no git spawn (core never
//! spawns); branch escalation lives on the binary side (`fael/src/refs.rs`).

use crate::{Log, Row, looks_like_id};

/// Union-scope existence of one token: exact id or case-insensitive prefix
/// over rows AND closes (a close row's own id counts — it exists, only the
/// row it points at is gone). `Many` is ambiguity, never missing: an
/// abbreviation that decayed as the log grew.
pub enum Ref<'a> {
    One(&'a Row),
    Many(Vec<&'a Row>),
    Missing,
}

/// Existence by exact id or case-insensitive prefix over `log.rows` then
/// `log.closes`. Empty tokens never match (like `resolve`'s guard).
pub fn ref_state<'a>(log: &'a Log, tok: &str) -> Ref<'a> {
    if tok.is_empty() {
        return Ref::Missing;
    }
    let mut hits: Vec<&Row> = vec![];
    for r in log.rows.iter().chain(log.closes.iter()) {
        if r.id
            .get(..tok.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(tok))
        {
            hits.push(r);
        }
    }
    match hits.len() {
        0 => Ref::Missing,
        1 => Ref::One(hits[0]),
        _ => Ref::Many(hits),
    }
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
/// `Missing` via `ref_state` — `One`/`Many` resolve, so real and ambiguous
/// citations never report. Pure, no spawn; the binary re-checks candidates
/// against unmerged branches once before reporting.
pub fn phantom_refs(log: &Log, text: &str) -> Vec<String> {
    id_tokens(text)
        .into_iter()
        .filter(|tok| matches!(ref_state(log, tok), Ref::Missing))
        .map(String::from)
        .collect()
}
