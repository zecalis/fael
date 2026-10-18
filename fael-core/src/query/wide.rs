//! Opt-in wide recall for `find --recall wide` (and MCP `recall: "wide"`).
//! When set, results are the union of (a) today's exact filter hits, (b)
//! rows sharing an exact key with any (a) hit, and (c) — when the query also
//! carries text — today's text match without the `--files` narrowing. Ranked
//! (a) first, then (b), then (c); each tier keeps today's rank. Default (no
//! flag) never reaches here — `select::find` routes there only on `wide`.

use super::Filter;
use super::matching::key_match;
use crate::{Log, Row, is_alias_row, is_carrier_row, to_matches};
use std::collections::HashSet;

/// Only `"wide"` widens; anything else is rejected at parse, never silent.
pub fn is_wide(recall: &Option<String>) -> bool {
    recall.as_deref() == Some("wide")
}

/// Validate the CLI `--recall` / MCP `recall` value into the `Filter` field.
pub fn parse_recall(v: Option<&str>) -> Result<Option<String>, String> {
    match v {
        None => Ok(None),
        Some("wide") => Ok(Some("wide".into())),
        Some(o) => Err(format!("rejected: --recall takes wide — got {o:?}")),
    }
}

/// The wide union for `f` (which carries `recall: Some("wide")`). Sub-filters
/// clear `recall` so the shared `find` takes the narrow path — no recursion.
pub fn find<'a>(log: &'a Log, f: &Filter) -> Vec<&'a Row> {
    let narrow = Filter {
        recall: None,
        ..f.clone()
    };
    let a = super::find(log, &narrow);
    let mut seen: HashSet<&str> = a.iter().map(|r| r.id.as_str()).collect();
    let keys: HashSet<&str> = a.iter().filter_map(|r| r.key.as_deref()).collect();
    let mut out = a;
    if !keys.is_empty() {
        let b = key_sharing(log, f, &keys, &seen);
        out.extend(b.iter().map(|r| {
            seen.insert(r.id.as_str());
            *r
        }));
    }
    if f.text.as_deref().is_some_and(|t| !t.trim().is_empty()) {
        let unfiled = Filter {
            files: vec![],
            recall: None,
            ..f.clone()
        };
        for r in super::find(log, &unfiled) {
            if seen.insert(r.id.as_str()) {
                out.push(r);
            }
        }
    }
    out
}

/// (b): open rows carrying one of `keys`, ignoring the files/text narrowing
/// but keeping every other explicit filter — the widening is about where a
/// row sits and what it says, never about kind/since/by/to/revisit.
fn key_sharing<'a>(
    log: &'a Log,
    f: &Filter,
    keys: &HashSet<&str>,
    seen: &HashSet<&str>,
) -> Vec<&'a Row> {
    let hide: HashSet<&str> = if f.all {
        HashSet::new()
    } else {
        let mut h: HashSet<&str> = super::closed(log)
            .union(&super::superseded(log))
            .copied()
            .collect();
        h.extend(log.dormant.iter().map(String::as_str));
        h
    };
    let revisit = f.revisit.as_ref().map(|q| q.to_lowercase());
    let rows: Vec<&Row> = log
        .rows
        .iter()
        .filter(|r| {
            !seen.contains(r.id.as_str())
                && !hide.contains(r.id.as_str())
                && !is_alias_row(r)
                && !is_carrier_row(r)
                && r.key.as_deref().is_some_and(|k| keys.contains(k))
                && f.kind.as_ref().is_none_or(|k| &r.kind == k)
                && f.by.as_ref().is_none_or(|b| &r.by == b)
                && f.to.as_ref().is_none_or(|t| {
                    r.to_who()
                        .is_some_and(|w| to_matches(w, t) || to_matches(t, w))
                })
                && f.since.as_ref().is_none_or(|s| r.ts.as_str() >= s.as_str())
                && f.key
                    .as_ref()
                    .is_none_or(|g| r.key.as_deref().is_some_and(|k| key_match(g, k)))
                && revisit.as_ref().is_none_or(|q| {
                    r.revisit()
                        .is_some_and(|v| q.is_empty() || v.to_lowercase().contains(q))
                })
        })
        .collect();
    super::ranked(rows, None, |_| 0, super::fresh_ts)
}
