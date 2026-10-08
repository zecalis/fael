//! The budget half of the noise contract (PLAN-fael-say-gate chunk 6): what
//! `say_within` cuts first over a budget. Split from `say_contract` for the
//! file-size cap; the fixtures and slots live there.

use super::say::{Line, Outbox};
use super::say_contract::{all, seen, slot};
use super::state::lock_seen;

/// PLAN-fael-say-gate chunk 6: rows and bodies keep their say; over
/// the budget the stashed notice goes first, then the edit hint, and a cut
/// line spends no key (a later push may say it).
#[test]
fn over_the_budget_the_notice_goes_then_the_hint_never_the_rows() {
    let pick = |slots: &[usize]| -> Vec<Line> {
        all()
            .into_iter()
            .filter(|l| slots.contains(&slot(&l.kind)))
            .collect()
    };
    let cost = |ls: &[Line]| -> usize { ls.iter().map(|l| crate::core::est_tokens(&l.text)).sum() };
    let lines = pick(&[0, 4, 2, 9]); // row, bodies, hint, notice
    let keep = cost(&pick(&[0, 4]));
    let run = |budget: usize| {
        let p = seen("s.seen");
        let mut out = Outbox::open(lock_seen(&p));
        out.say_within(budget, lines.clone());
        let said = out.reply().context().unwrap_or("").to_string();
        let keys = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        (said, keys)
    };
    let all_said = run(usize::MAX).0;
    assert!(all_said.contains("a stashed line") && all_said.contains("fael close 01ASK"));
    let (said, keys) = run(keep + cost(&pick(&[2])));
    assert!(
        !said.contains("a stashed line") && said.contains("fael close 01ASK"),
        "{said}"
    );
    let (said, keys_cut) = run(keep);
    assert!(
        !said.contains("fael close 01ASK") && !said.contains("a stashed line"),
        "{said}"
    );
    assert!(said.contains("bodies:") && said.contains("01ROW"), "{said}");
    // the cut hint kept its key
    assert!(
        keys.contains("~01ASK") && !keys_cut.contains("~01ASK"),
        "{keys} / {keys_cut}"
    );
    // no budget at all still says the rows and the bodies line
    assert!(run(0).0.contains("bodies:"));
}

/// Over the budget the cut order is the notice, the consolidate ask, the
/// gone-check ask, then the edit hint — each whole, a cut line spending no key.
#[test]
fn over_the_budget_the_cut_order_is_notice_merge_check_then_ask() {
    let pick = |slots: &[usize]| -> Vec<Line> {
        all()
            .into_iter()
            .filter(|l| slots.contains(&slot(&l.kind)))
            .collect()
    };
    let cost = |ls: &[Line]| -> usize { ls.iter().map(|l| crate::core::est_tokens(&l.text)).sum() };
    let lines = pick(&[0, 2, 6, 8, 9]); // row, hint, merge, check, notice
    let said_at = |budget: usize| {
        let p = seen("s.seen");
        let mut out = Outbox::open(lock_seen(&p));
        out.say_within(budget, lines.clone());
        let said = out.reply().context().unwrap_or("").to_string();
        let keys = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        (said, keys)
    };
    let has = |said: &str| {
        ["a stashed line", "01MERGE", "01CHK", "fael close 01ASK"].map(|t| said.contains(t))
    };
    assert_eq!(has(&said_at(usize::MAX).0), [true; 4]);
    let kept = |slots: &[usize]| cost(&pick(slots));
    // each budget keeps one line more than the one below it
    assert_eq!(
        has(&said_at(kept(&[0, 2, 6, 8])).0),
        [false, true, true, true]
    );
    assert_eq!(
        has(&said_at(kept(&[0, 2, 8])).0),
        [false, false, true, true]
    );
    let (said, keys) = said_at(kept(&[0, 2]));
    assert_eq!(has(&said), [false, false, false, true], "{said}");
    // the cut check kept its key for a later push
    assert!(
        !keys.contains("~check:01CHK") && keys.contains("~01ASK"),
        "{keys}"
    );
}
