//! The friction line's top-session suffix (issue 01M88QVV).

use super::top_suffix;
use fael_core as core;

fn tally(calls: usize, rejects: usize) -> core::stats::Tally {
    core::stats::Tally {
        calls,
        rejects,
        ..Default::default()
    }
}

/// One session with 513 of 580 rejects is named with the rate without it;
/// a session under a quarter of the friction leaves the line as it was.
#[test]
fn a_session_holding_a_quarter_of_the_friction_is_named() {
    let all = tally(1000, 580);
    assert_eq!(
        top_suffix(&all, &tally(600, 513)),
        " (top session 513 of 580, 88%; without it 16.8/100)"
    );
    assert_eq!(top_suffix(&all, &tally(100, 100)), "");
    assert_eq!(top_suffix(&tally(5, 0), &tally(0, 0)), "", "no friction");
    assert_eq!(
        top_suffix(&tally(3, 3), &tally(3, 3)),
        " (top session 3 of 3, 100%; without it n/a)"
    );
}
