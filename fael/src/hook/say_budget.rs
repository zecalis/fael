//! The budget half of the noise contract (PLAN-fael-say-gate chunk 6): what
//! `say_within` cuts first over a budget. Split from `say_contract` for the
//! file-size cap; the fixtures and slots live there.

use super::say::{Line, Outbox};
use super::say_card::At;
use super::say_contract::{all, seen, slot};
use super::state::lock_seen;

/// PLAN-fael-say-gate chunk 6: rows keep their say; over
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
    let lines = pick(&[0, 2, 5]); // row, hint, notice
    let keep = cost(&pick(&[0]));
    let run = |budget: usize| {
        let p = seen("s.seen");
        let mut out = Outbox::open(At::Edit, lock_seen(&p));
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
    assert!(said.contains("01ROW"), "{said}");
    // the cut hint kept its key
    assert!(
        keys.contains("~01ASK") && !keys_cut.contains("~01ASK"),
        "{keys} / {keys_cut}"
    );
    // no budget at all still says the rows
    assert!(run(0).0.contains("01ROW"));
}

/// Push lines hold their cut ranks: rows + Overlap + BriefDrift over
/// `push_tokens` stays within budget with the rows intact; a commit reply
/// with Cited + Checkpoint over budget cuts in card order (Cited first).
#[test]
fn rows_survive_overlap_and_drift_and_a_commit_reply_cuts_in_card_order() {
    use crate::core::est_tokens;
    let pick = |slots: &[usize]| -> Vec<Line> {
        all()
            .into_iter()
            .filter(|l| slots.contains(&slot(&l.kind)))
            .collect()
    };
    let cost = |ls: &[Line]| -> usize { ls.iter().map(|l| est_tokens(&l.text)).sum() };
    // rows + overlap + drift at the edit: the push stays within budget with
    // the rows intact (rows are Never-cut, overlap Last(6), drift First(7)).
    let lines = pick(&[0, 7, 10]);
    let keep = cost(&pick(&[0]));
    let run_edit = |budget: usize| {
        let p = seen("s.seen");
        let mut out = Outbox::open(At::Edit, lock_seen(&p));
        out.say_within(budget, lines.clone());
        let said = out.reply().context().unwrap_or("").to_string();
        let keys = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        (said, keys)
    };
    let all_said = run_edit(usize::MAX).0;
    assert!(all_said.contains("01ROW"), "{all_said}");
    assert!(all_said.contains("fael chunk note 01D"), "{all_said}");
    assert!(all_said.contains("fael chunk show 01C"), "{all_said}");
    let (said, keys_cut) = run_edit(keep);
    assert!(said.contains("01ROW"), "{said}");
    assert!(!said.contains("fael chunk note 01D"), "{said}");
    assert!(!said.contains("fael chunk show 01C"), "{said}");
    assert!(est_tokens(&said) <= keep, "{said}");
    // the cut lines kept their keys for a later push
    assert!(
        !keys_cut.contains("~overlap:01D:a.rs") && !keys_cut.contains("~drift:01C@7"),
        "{keys_cut}"
    );
    // a commit reply with Cited + Checkpoint over budget cuts in card order:
    // Cited (First(3)) goes before Checkpoint (First(4)).
    let commit = pick(&[3, 9]);
    let checkpoint_cost = cost(&pick(&[9]));
    let run_search = |budget: usize| {
        let p = seen("s.seen");
        let mut out = Outbox::open(At::Search, lock_seen(&p));
        out.say_within(budget, commit.clone());
        let said = out.reply().context().unwrap_or("").to_string();
        let keys = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        (said, keys)
    };
    let both = run_search(usize::MAX).0;
    assert!(
        both.contains("01CITED") && both.contains("checkpoint"),
        "{both}"
    );
    let (said, keys_cut) = run_search(checkpoint_cost);
    assert!(said.contains("checkpoint"), "{said}");
    assert!(!said.contains("01CITED"), "{said}");
    assert!(est_tokens(&said) <= checkpoint_cost, "{said}");
    assert!(!keys_cut.contains("~cited:01CITED"), "{keys_cut}");
}
