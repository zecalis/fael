//! The decision record of one push (SPEC-fael-learn-loop §A): why it fired,
//! which policy decided, which rows it said and which it cut — and why. It
//! rides the push's usage line; what the agent sees is untouched.

use crate::core;
use serde_json::{Value, json};
use std::collections::HashSet;

/// Cut rows one line records; `cut_n` still counts them all, so the log never
/// becomes a payload dump.
const MAX_CUT: usize = 20;

/// The decision record of a push that said `said` of `sel.shown`. `trigger`
/// is what made the push fire (`read`, `edit`, `shell-edit`, or a search's
/// `reader-arg` / `hitlist` / `glob`). Rows past `said` are the token
/// budget's cut; `select`'s own cuts carry their reason. `touched` is the
/// session's working set before this push (`None` = no session): each row's
/// `feat.touch` counts its files in it, and the said rows `touch@1` would have
/// dropped ride `would_drop` — recorded only, the agent still saw them.
pub(crate) fn record(
    trigger: &str,
    sel: &core::Selection,
    said: usize,
    focus: &core::Focus,
    touched: Option<&HashSet<String>>,
) -> Value {
    let said = said.min(sel.shown.len());
    let budget = sel.shown[said..]
        .iter()
        .enumerate()
        .map(|(i, r)| (*r, sel.tier(said + i), core::CUT_BUDGET));
    let cut: Vec<_> = sel.cut.iter().copied().chain(budget).collect();
    let now = core::now_ms();
    let mut feat = serde_json::Map::new();
    let mut would_drop = vec![];
    let kept = sel.shown[..said]
        .iter()
        .enumerate()
        .map(|(i, r)| (*r, sel.tier(i)));
    for (i, (r, tier)) in kept
        .chain(cut.iter().take(MAX_CUT).map(|(r, t, _)| (*r, *t)))
        .enumerate()
    {
        let age = core::ulid_ms(&r.id).map_or(0, |ms| now.saturating_sub(ms) / 86_400_000);
        let mut f = json!({
            "tier": tier,
            // a File row of a hub push — the rows the hub rule can cut
            "hub": sel.hub && core::bucket(r, tier, focus) == core::Bucket::File,
            "kind": r.kind,
            "age_d": age,
        });
        if let Some(set) = touched {
            let touch = r.files.iter().filter(|p| set.contains(*p)).count();
            f["touch"] = touch.into();
            // only rows the push said can be dropped from what the agent saw
            if i < said && core::touch_drops(r, touch) {
                would_drop.push(r.id.clone());
            }
        }
        feat.insert(r.id.clone(), f);
    }
    let mut d = json!({
        "trigger": trigger,
        "policy": core::BASELINE.name(),
        "feat": feat,
    });
    if !would_drop.is_empty() {
        d["would_drop"] = json!({"policy": core::TOUCH.name(), "ids": would_drop});
    }
    if !cut.is_empty() {
        d["cut"] = cut
            .iter()
            .take(MAX_CUT)
            .map(|(r, _, why)| json!({"id": r.id, "r": why}))
            .collect();
        d["cut_n"] = cut.len().into();
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_is_whole_days_since_the_row_id() {
        let old = core::Row {
            id: core::ulid_at(core::now_ms() - 3 * 86_400_000 - 5_000),
            kind: "note".into(),
            ..core::Row::default()
        };
        let policy = core::PushPolicy {
            max_rows: 5,
            budget: 800,
            background: core::PUSH_BACKGROUND,
        };
        let focus = core::Focus::default();
        let sel = core::select(vec![(&old, 0)], &focus, &policy);
        let d = record("read", &sel, 1, &focus, None);
        assert_eq!(d["feat"][&old.id]["age_d"], 3, "{d}");
        assert_eq!(d["feat"][&old.id]["kind"], "note", "{d}");
        // the token budget cut the one row: said 0, it is a budget cut
        let d = record("read", &sel, 0, &focus, None);
        assert_eq!(d["cut"], json!([{"id": old.id, "r": "budget"}]), "{d}");
    }
}
