//! The yield tally one kind keeps (`said.rs` fills it): entries said, entries
//! acted on, tokens the entries cost, and the same split by the hook event
//! that said them. Split out of `said.rs` for the file-size limit.

use super::acted::ActedCounts;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct KindYield {
    /// `said` entries of this kind: one per row id, key or file set.
    pub said: usize,
    /// Of those, acted on later in the same session.
    pub earned: usize,
    /// Tokens the kind's lines cost: the sum of `t` over its entries (a line's
    /// `est_tokens` shared among its entries). Only usage written since `t`
    /// existed counts, an older entry adds 0 — so it is no total for a window
    /// that reaches back before it. Absent from the JSON when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tokens: usize,
    /// The same three numbers per hook event that said the kind (`edit`,
    /// `shell-edit`, `search`, `session-start`, …), keyed by the usage line's
    /// `event`; earned by the kind's own rule, so the events sum to the kind.
    /// Absent from the JSON when nothing was said.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_event: BTreeMap<String, EventYield>,
    /// Stamped retires in the window (r2, `held` only): per `shown` source
    /// (`held`, `done`, later `add`/`start`) the retire events whose row
    /// names that source, split close / supersede / bump. Empty on every
    /// other kind — and on `held` when nothing stamped yet.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub acted: BTreeMap<String, ActedCounts>,
}

/// A kind's yield at one hook event.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EventYield {
    pub said: usize,
    pub earned: usize,
    /// As `KindYield::tokens`; absent from the JSON when 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tokens: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// One more `said` entry of `bucket`, said at `event` (`None` on a usage line
/// with none), earned or not, costing `tokens`.
pub(super) fn tally(
    out: &mut BTreeMap<String, KindYield>,
    bucket: &str,
    event: Option<&str>,
    earned: bool,
    tokens: usize,
) {
    let y = out.entry(bucket.to_string()).or_default();
    y.said += 1;
    y.earned += earned as usize;
    y.tokens += tokens;
    if let Some(e) = event {
        let e = y.by_event.entry(e.to_string()).or_default();
        e.said += 1;
        e.earned += earned as usize;
        e.tokens += tokens;
    }
}
