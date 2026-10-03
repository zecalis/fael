//! The session's newest-row clock, shared by the hooks and stats — pure.

use crate::Log;
use crate::id::ts_ms;

/// The newest add or close row stamped at or after `since_ms`, in ms. Numeric
/// on both sides — a whole-second string compare reads a row filed just
/// before the session start as newer whenever they share a second.
pub fn last_row_ms(log: &Log, since_ms: i64) -> Option<i64> {
    log.rows
        .iter()
        .chain(log.closes.iter())
        .filter_map(|r| ts_ms(&r.ts))
        .filter(|&ms| ms >= since_ms)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Row;

    #[test]
    fn new_row_compares_ms_not_seconds() {
        let mut log = Log::default();
        let mut r = Row::new("t-0000", "note", "x", vec!["a.rs".into()]);
        r.ts = "2026-09-25T10:00:01Z".into();
        log.rows.push(r);
        assert!(last_row_ms(&log, ts_ms("2026-09-25T10:00:01.500Z").unwrap()).is_none());
        assert!(last_row_ms(&log, ts_ms("2026-09-25T10:00:01Z").unwrap()).is_some());
    }
}
