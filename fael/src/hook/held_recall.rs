//! Closed-issue recall at chunk start (e1 same-bug link), split out of
//! `held.rs` at the 400-line ratchet: closed issues with files under the
//! scope, plus rows under the scope that `--follows` a closed issue on
//! another surface. Newest first, cap `RECALL_MAX`, file sets only — never
//! text inference (decision 01M5ABTD: the cross-surface link comes from the
//! agent's own `--follows`, not from fael guessing). Silent when none. Never
//! blocks start, never closes anything.

use crate::core;

/// At most this many closed rows; the rest stay silent, not counted.
const RECALL_MAX: usize = 2;

/// One recall line per row, with the `--follows` link the next agent uses.
pub(crate) fn recall_text(r: &crate::Repo, scope: &[String]) -> String {
    if scope.is_empty() {
        return String::new();
    }
    let mut log = crate::read(r);
    super::changed::mark_dormant(r, &mut log);
    let (closed, gone) = (core::closed(&log), core::superseded(&log));
    let under = |files: &[String]| {
        files
            .iter()
            .any(|f| crate::core::work::overlap(std::slice::from_ref(f), scope))
    };
    let ab = core::abbrev(&log);
    // rows under the scope that continue a closed issue elsewhere
    let mut following: Vec<(&core::Row, &core::Row)> = vec![];
    for w in &log.rows {
        let Some(t) = w.follows.as_deref() else {
            continue;
        };
        if !closed.contains(t) || gone.contains(w.id.as_str()) {
            continue;
        }
        if log.dormant.contains(&w.id) || !under(&w.files) {
            continue;
        }
        if let Some(target) = log.rows.iter().find(|x| x.id == t) {
            following.push((w, target));
        }
    }
    // a closed issue its continuation already names is not named twice
    let continued: std::collections::HashSet<&str> =
        following.iter().map(|(_, t)| t.id.as_str()).collect();
    let mut shut: Vec<&core::Row> = log
        .rows
        .iter()
        .filter(|w| {
            w.kind == "issue"
                && closed.contains(w.id.as_str())
                && !gone.contains(w.id.as_str())
                && !continued.contains(w.id.as_str())
                && !log.dormant.contains(&w.id)
                && under(&w.files)
        })
        .collect();
    shut.sort_by(|a, b| b.id.cmp(&a.id));
    following.sort_by(|a, b| b.0.id.cmp(&a.0.id));
    let mut out = String::new();
    let mut n = 0;
    for w in shut.into_iter().take(RECALL_MAX) {
        if n >= RECALL_MAX {
            break;
        }
        let short = ab.short(&w.id);
        let file = w
            .files
            .first()
            .map(String::as_str)
            .unwrap_or(scope[0].as_str());
        out.push_str(&format!(
            "closed earlier under this scope (left closed): {short} \"{}\" — same bug? continue it: fael add issue \"<what broke>\" --files {file} --follows {short}\n",
            w.display_title()
        ));
        n += 1;
    }
    for (w, t) in following.into_iter().take(RECALL_MAX.saturating_sub(n)) {
        let (fs, cs) = (ab.short(&w.id), ab.short(&t.id));
        out.push_str(&format!(
            "same bug, new surface (left closed): {cs} \"{}\" — continued by {fs} \"{}\": fael find {cs} (file it with --follows {cs})\n",
            t.display_title(),
            w.display_title()
        ));
    }
    out
}
