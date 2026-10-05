//! Coverage (SPEC-fael-learn-loop §E): many pushes are not many independent
//! observations. A section's search pushes must spread over days and sessions.
//! The thresholds were set before `tune` existed (decision 01M45DKB4) and
//! `tune` never picks them: it reports the numbers and whether they pass.

use serde::Serialize;
use std::collections::HashMap;

pub const MIN_DAYS: usize = 3;
pub const MAX_DAY_SHARE_PCT: usize = 50;
pub const MAX_SESSION_SHARE_PCT: usize = 10;

/// A search push, reduced to what coverage and arm sizes count.
pub struct Push<'a> {
    pub repo: &'a str,
    pub client: &'a str,
    pub session: &'a str,
    pub day: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Coverage {
    pub sessions: usize,
    pub pushes: usize,
    pub distinct_days: usize,
    /// Busiest local day's share of the pushes, percent.
    pub max_day_share_pct: f64,
    pub max_session_share_pct: f64,
    /// Against the provisional thresholds above.
    pub passes: bool,
}

pub fn coverage(pushes: &[&Push]) -> Coverage {
    let mut days: HashMap<i64, usize> = HashMap::new();
    let mut sessions: HashMap<(&str, &str), usize> = HashMap::new();
    for p in pushes {
        *days.entry(p.day).or_default() += 1;
        *sessions.entry((p.repo, p.session)).or_default() += 1;
    }
    let n = pushes.len();
    let share = |m: Option<usize>| match (m, n) {
        (Some(m), n) if n > 0 => 100.0 * m as f64 / n as f64,
        _ => 0.0,
    };
    let (day_share, session_share) = (
        share(days.values().max().copied()),
        share(sessions.values().max().copied()),
    );
    Coverage {
        sessions: sessions.len(),
        pushes: n,
        distinct_days: days.len(),
        max_day_share_pct: day_share,
        max_session_share_pct: session_share,
        passes: n > 0
            && days.len() >= MIN_DAYS
            && day_share <= MAX_DAY_SHARE_PCT as f64
            && session_share <= MAX_SESSION_SHARE_PCT as f64,
    }
}

/// `YYYY-MM-DD` of a day number (days since the epoch).
pub fn civil(day: i64) -> String {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push(session: &str, day: i64) -> Push<'_> {
        Push {
            repo: "/r",
            client: "claude",
            session,
            day,
        }
    }

    #[test]
    fn civil_dates_round_trip_known_days() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(20_731), "2026-10-05");
        assert_eq!(civil(-1), "1969-12-31");
    }

    #[test]
    fn many_pushes_from_one_day_or_one_session_do_not_cover() {
        let names: Vec<String> = (0..34).map(|i| format!("s{i}")).collect();
        // Mon 30 / Tue 2 / Wed 2: three days, one carries the evidence
        let ps: Vec<Push> = (0..34)
            .map(|i| push(&names[i], if i < 30 { 1 } else { 2 + i as i64 % 2 }))
            .collect();
        let c = coverage(&ps.iter().collect::<Vec<_>>());
        assert_eq!((c.distinct_days, c.sessions), (3, 34));
        assert!(c.max_day_share_pct > 80.0 && !c.passes, "{c:?}");
        // an even spread passes
        let spread: Vec<Push> = (0..30).map(|i| push(&names[i], i as i64 % 3)).collect();
        assert!(coverage(&spread.iter().collect::<Vec<_>>()).passes);
        // spread over days, but one session holds a quarter of the pushes
        let mut ps = spread;
        ps.extend((0..10).map(|i| push("big", i % 3)));
        let c = coverage(&ps.iter().collect::<Vec<_>>());
        assert!(c.max_session_share_pct > 10.0 && !c.passes, "{c:?}");
        assert!(!coverage(&[]).passes);
    }
}
