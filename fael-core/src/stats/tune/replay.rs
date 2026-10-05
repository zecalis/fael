//! Replay a candidate over what happened: each said search row, in the order
//! it was said, asked "would you have kept this?" with only what the policy
//! could have known then. Cut-only candidates: a replay never adds a row.

use super::Ob;
use crate::query::{YIELD_MIN_N, touch_drops, touch_yield_drops};
use std::collections::HashMap;

/// Where a `touch-yield@1` decision got its history: the level that had
/// enough earlier sessions (0 = row,file,trigger … 3 = class), or 4 = none had.
pub type Used = [usize; 5];

/// `touch@1` over each row: the pinned rule, called as the shadow called it.
pub(super) fn touch(obs: &[&Ob]) -> Vec<bool> {
    obs.iter()
        .map(|o| match (o.row, o.touch) {
            (Some(r), Some(t)) => touch_drops(r, t),
            _ => false,
        })
        .collect()
}

/// The history keys a said row looks itself up under, narrowest first.
fn keys(o: &Ob) -> [Option<String>; 4] {
    let id = o.o.id;
    [
        Some(format!("{id}|{}|{}", o.file, o.trigger)),
        Some(format!("{id}|{}", o.file)),
        Some(id.to_string()),
        o.class.clone(),
    ]
}

/// `touch-yield@1` over each row. A row's history is the engaged outcomes of
/// sessions that had *ended* before this row was said (a session still running
/// has no outcome yet), at most the last `window` of them per key.
pub(super) fn touch_yield(obs: &[&Ob], window: Option<usize>) -> (Vec<bool>, Used) {
    let n = obs.len();
    let mut by_said: Vec<usize> = (0..n).collect();
    by_said.sort_by_key(|&i| obs[i].o.first_said_ms);
    let mut by_end = by_said.clone();
    by_end.sort_by_key(|&i| obs[i].o.end_ms);
    let mut hist: HashMap<String, Vec<bool>> = HashMap::new();
    let (mut learned, mut used) = (0, [0; 5]);
    let mut drops = vec![false; n];
    for &i in &by_said {
        while learned < n && obs[by_end[learned]].o.end_ms < obs[i].o.first_said_ms {
            let o = obs[by_end[learned]];
            for k in keys(o).into_iter().flatten() {
                hist.entry(k).or_default().push(o.engaged);
            }
            learned += 1;
        }
        let (Some(r), Some(t)) = (obs[i].row, obs[i].touch) else {
            continue;
        };
        if !touch_drops(r, t) {
            continue; // history only ever holds a row back from a `touch@1` drop
        }
        let level = keys(obs[i]).into_iter().enumerate().find_map(|(l, k)| {
            let v = hist.get(&k?)?;
            let v = &v[v.len().saturating_sub(window.unwrap_or(usize::MAX))..];
            (v.len() >= YIELD_MIN_N).then(|| (l, v.iter().filter(|e| **e).count(), v.len()))
        });
        used[level.map_or(4, |l| l.0)] += 1;
        drops[i] = touch_yield_drops(r, t, level.map(|(_, x, n)| (x, n)));
    }
    (drops, used)
}
