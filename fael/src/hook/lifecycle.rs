//! The lifecycle panel's disk half (decision 01M5528X): which open rows are *held* —
//! some but not all stamped files changed, what the Held line asks about. The counts
//! themselves are `core::stats::lifecycle`.

use super::changed::{Blobs, STAMP_MAX_BYTES, file_verdict, mark_dormant, resolve};
use crate::core;
use std::collections::HashSet;

/// This repo's rows by state per kind, read now: the log, the disk, `fael.db`.
pub(crate) fn of(r: &crate::Repo) -> core::stats::Lifecycle {
    let mut log = crate::read(r);
    mark_dormant(r, &mut log);
    let al = crate::aliases::load(r, &log, false);
    let mut blobs = Blobs::new();
    let held: HashSet<String> = log
        .rows
        .iter()
        .filter(|w| !log.dormant.contains(&w.id) && is_held(w, r, &al, &mut blobs))
        .map(|w| w.id.clone())
        .collect();
    core::stats::lifecycle(&log, &held)
}

/// Some stamped file changed and the row is not dormant (not every one did). An issue
/// keeps its own edit ask and a claimed row is work in progress — never held.
fn is_held(w: &core::Row, r: &crate::Repo, al: &core::Aliases, blobs: &mut Blobs) -> bool {
    w.kind != "issue"
        && w.held().is_none()
        && w.file_hashes().is_some_and(|fh| {
            fh.iter().any(|(f, v)| {
                let (Some(want), Some(target)) = (v.as_str(), resolve(al, &r.root, f)) else {
                    return false;
                };
                file_verdict(want, &target, &r.root, blobs, STAMP_MAX_BYTES) == Some(true)
            })
        })
}

/// The panel `fael stats` leads with: one line per kind, `decisions: live 4 · held 1 ·
/// dormant 7 · retired 12`. No rows at all = no panel.
pub(super) fn print(l: &core::stats::Lifecycle) {
    if l.is_empty() {
        return;
    }
    println!("rows (this repo):");
    for (kind, s) in [
        ("decision", &l.decision),
        ("issue", &l.issue),
        ("note", &l.note),
    ] {
        println!(
            "  {kind}: live {} · held {} · dormant {} · retired {}",
            s.live, s.held, s.dormant, s.retired
        );
    }
}
