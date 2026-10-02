//! The prompt key hint: open keys a user prompt names, ranked by match
//! strength — the one part of `lookup` about injecting pointers, not paging.

use super::{KeyUse, select};
use crate::Log;

/// Open keys one matched segment may name before it is an area word, not a
/// topic — and the most keys one hint line lists.
const HINT_MAX_KEYS: usize = 3;

/// Open keys a user prompt names (01M3WCK7N), ranked by match strength first
/// (01M3XKB7E): a key named whole in the prompt (`plan:x-y:handoff`, word
/// boundary respected) comes first; otherwise the key still only counts
/// through its head — the first segment after the namespace, exact, ASCII
/// case-insensitive (`credit` in `vela:credit-ledger`) — and every prompt
/// word matching a further segment raises its rank. Never fuzzy: no prefix,
/// stem or edit distance, and a word under 4 chars or all digits never
/// matches. The old >3-open-keys drop is gone (it silenced a topic as it
/// grew — a 4th `vela:credit-*` key made "credit" hint nothing): strongest
/// matches win the line instead, and a plain English word that happens to be
/// a head ("file" → `fael:file-size`) is stop-listable later from the usage
/// journal, which now records which word fired (01M3XGY8, 01M3XKB7G).
/// Pairs each `KeyUse` with the prompt words that named it, lowercased, in
/// key order — the hint line shows them, so a misfire is judgeable.
/// The namespace never matches: in vela "data scope" hit every `vela:*-scope`
/// key and "fael" hit `fael:store`.
/// Open = some row on the key is neither closed nor superseded. At most
/// `HINT_MAX_KEYS`, best match first, most used first as the tie-break.
pub fn key_hints(log: &Log, prompt: &str) -> Vec<(KeyUse, Vec<String>)> {
    let (closed, gone) = (select::closed(log), select::superseded(log));
    let keys = super::lookup::tally(log.rows.iter().filter(|r| {
        !closed.contains(r.id.as_str())
            && !gone.contains(r.id.as_str())
            && !crate::is_carrier_row(r)
            && !crate::is_alias_row(r)
    }));
    let words: std::collections::HashSet<String> = prompt
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 4 && !w.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_ascii_lowercase)
        .collect();
    // nested fns, not closures: a returning closure would need lifetimes spelled out
    fn segs(key: &str) -> Vec<String> {
        key.split_once(':')
            .map_or(key, |(_, t)| t)
            .split([':', '-', '_', '.', '/'])
            .map(|s| s.to_ascii_lowercase())
            .collect()
    }
    let mut hit: Vec<(KeyUse, Vec<String>, bool)> = vec![];
    for k in &keys {
        let s = segs(&k.key);
        let named: Vec<String> = s
            .iter()
            .filter(|seg| words.contains(*seg))
            .cloned()
            .collect();
        // the key typed whole (`plan:x-y:handoff`) counts by itself — the
        // agent named it in full, no head rule should stand in the way
        let whole = names_key(prompt, &k.key);
        // otherwise the head must match for the key to count at all — the
        // trailing segments only ever raise its rank
        let head_named = s.first().is_some_and(|h| named.iter().any(|n| n == h));
        if !whole && !head_named {
            continue;
        }
        hit.push((k.clone(), named, whole));
    }
    // best match first (01M3XKB7E): the key named whole, then most prompt
    // words matched, then most used like `keys`, then name
    hit.sort_by(|(a, an, aw), (b, bn, bw)| {
        bw.cmp(aw)
            .then_with(|| bn.len().cmp(&an.len()))
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.key.cmp(&b.key))
    });
    hit.truncate(HINT_MAX_KEYS);
    hit.into_iter().map(|(k, named, _)| (k, named)).collect()
}

/// Does the prompt name the key whole: the exact key text at a word boundary
/// (a neighbour char that is not alphanumeric), case-insensitive. Substring
/// inside a longer word never counts (`hooks.json` does not name `hook`).
fn names_key(prompt: &str, key: &str) -> bool {
    let lc = key.to_ascii_lowercase();
    let p = prompt.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = p[at..].find(&lc) {
        let i = at + i;
        let edge = |c: Option<char>| c.is_none_or(|c| !c.is_ascii_alphanumeric());
        if edge(p[..i].chars().next_back()) && edge(p[i + lc.len()..].chars().next()) {
            return true;
        }
        at = i + lc.len();
    }
    false
}
