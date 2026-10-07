//! The same-burst window: two adds from one writer inside it are parallel
//! calls, not a replacement (01M47QEJ). Split out of `decide.rs` (file-size
//! ratchet) — no verdict logic here, only the clock.

use crate::core;

/// Two adds from one writer inside `BURST_MS` are parallel calls, not a
/// replacement (01M47QEJ): every sub-second edge on record hid a different
/// topic, while every edge ≥8 s old reads as a real replacement.
const BURST_MS: i64 = 1_000;

/// The burst window; `FAEL_BURST_MS` overrides it so a test pins the
/// past-window case with `0` instead of sleeping out a real second.
fn burst_ms() -> i64 {
    std::env::var("FAEL_BURST_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(BURST_MS)
}

/// The candidate was filed less than a burst before the new row — hold, never
/// act. Unparseable timestamps read as no burst: old rows keep acting.
pub(super) fn burst(old: &core::Row, row: &core::Row) -> bool {
    match (core::ts_ms(&old.ts), core::ts_ms(&row.ts)) {
        (Some(o), Some(n)) => 0 <= n - o && n - o < burst_ms(),
        _ => false,
    }
}
