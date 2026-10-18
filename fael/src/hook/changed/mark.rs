//! `log.dormant` / `log.retired` fill, split out of changed.rs at the
//! 400-line ratchet: the two entry points over the same body.

use crate::core;
use std::collections::HashSet;

/// Fill `log.dormant` for a pull (`find`, `kickoff`, session start, chunk
/// start): every open row checked at the stamp-side cap, one blob cache.
pub(crate) fn mark_dormant(r: &crate::Repo, log: &mut core::Log) {
    let al = crate::aliases::load(r, log, false);
    mark_dormant_with(r, log, &al);
}

/// `mark_dormant` over caller-supplied aliases — the write path (`fael add`)
/// passes `aliases::cached`, so it fills dormant/retired without building the
/// rename cache (`mark_dormant` would, and a build there freezes pre-rename
/// state a later hook edit trusts instead of rebuilding — the aliases rename
/// tests).
pub(crate) fn mark_dormant_with(r: &crate::Repo, log: &mut core::Log, al: &core::Aliases) {
    let hide: HashSet<&str> = core::closed(log)
        .union(&core::superseded(log))
        .copied()
        .collect();
    let mut blobs = super::Blobs::new();
    let ids: HashSet<String> = log
        .rows
        .iter()
        .filter(|w| !hide.contains(w.id.as_str()))
        .filter(|w| super::dormant(w, &r.root, al, &mut blobs, super::STAMP_MAX_BYTES))
        .map(|w| w.id.clone())
        .collect();
    // retired notes (chunk done, branch merged) are off the live path like dormant ones
    let retired = crate::hook::retired::ids(r, log, true);
    log.dormant = ids.union(&retired).cloned().collect();
    log.retired = retired;
}
