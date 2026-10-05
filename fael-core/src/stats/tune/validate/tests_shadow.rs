use super::*;
use crate::stats::tune::{PolicyResult, Section, Sizes};

/// A shadow section whose replay clears every bar; a test breaks one.
fn shadow() -> Section {
    let kept = Rate::new(90, 100);
    Section {
        sizes: Sizes {
            sessions: 40,
            search_pushes: 400,
            ..Sizes::default()
        },
        policies: vec![PolicyResult {
            policy: TOUCH.name(),
            dropped: 250,
            exposure: Rate::new(500, 1000), // half still said: exposure down 50%
            retained: Retained {
                cited: kept,
                pulled: kept,
                acted: kept,
            },
            ..PolicyResult::default()
        }],
        coverage: Coverage {
            passes: true,
            ..Coverage::default()
        },
        ..Section::default()
    }
}

#[test]
fn the_shadow_replay_promotes_holds_or_rolls_back() {
    let v = shadow_verdict(&shadow());
    assert_eq!((v.result, v.why), ("validated", vec![]));
    // too little data holds, never fails
    let mut s = shadow();
    s.sizes.sessions = 29;
    assert_eq!(shadow_verdict(&s).result, "insufficient_data");
    let mut s = shadow();
    s.policies[0].dropped = 199;
    assert_eq!(shadow_verdict(&s).result, "insufficient_data");
    let mut s = shadow();
    s.coverage.passes = false;
    assert_eq!(shadow_verdict(&s).result, "insufficient_data");
    // enough data, a bar missed: it rolls back and names the bar
    let mut s = shadow();
    s.policies[0].exposure = Rate::new(700, 1000);
    let v = shadow_verdict(&s);
    assert_eq!(v.result, "not_validated");
    assert!(v.why[0].contains("exposure"), "{:?}", v.why);
    let mut s = shadow();
    s.policies[0].retained.pulled = Rate::new(80, 100);
    assert!(shadow_verdict(&s).why[0].contains("pulled"));
    // no replay of touch@1 at all is no data
    assert_eq!(
        shadow_verdict(&Section::default()).result,
        "insufficient_data"
    );
}
