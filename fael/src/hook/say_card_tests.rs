//! The card half of the noise contract: what each kind's card in `say_card`
//! promises, looped over the fixtures of `say_contract` (split from it for the
//! file-size cap).

use super::say::Outbox;
use super::say_card::{At, Cut, policy};
use super::say_contract::all;
use crate::core::SayKind;

/// The card's `said` name is the one stats tallies by: it reads back through
/// `SayKind::parse`, is listed in `SayKind::ALL`, and no two kinds share it.
#[test]
fn every_kinds_said_name_round_trips_through_say_kind() {
    let mut seen_names = vec![];
    for l in all() {
        let said = policy(&l.kind).said;
        assert_eq!(SayKind::parse(said.name()), Some(said), "{:?}", l.kind);
        assert!(SayKind::ALL.contains(&said), "{:?}", l.kind);
        assert!(!seen_names.contains(&said), "{said:?} named by two kinds");
        seen_names.push(said);
    }
}

/// Every hook event, in the order `At` lists them.
const EVENTS: [At; 5] = [
    At::SessionStart,
    At::Edit,
    At::ShellEdit,
    At::Search,
    At::Review,
];

/// A kind is sayable at exactly the events its card names, and each kind
/// has at least one.
#[test]
fn a_line_is_sayable_only_at_the_events_of_its_card() {
    for l in all() {
        let card = policy(&l.kind);
        assert!(!card.at.is_empty(), "{:?} is said nowhere", l.kind);
        for at in EVENTS {
            let out = Outbox::open(at, None);
            assert_eq!(
                out.sayable(&l),
                card.at.contains(&at),
                "{:?} at {at:?}",
                l.kind
            );
        }
    }
}

/// `say` at an event off the card says nothing: a debug build also stops on
/// the caller's bug, a release build drops the line.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "is not said at"))]
fn a_line_said_at_another_event_is_dropped() {
    for l in all() {
        let off = EVENTS.into_iter().find(|a| !policy(&l.kind).at.contains(a));
        let mut out = Outbox::open(
            off.expect("every kind has an event it is not said at"),
            None,
        );
        out.say(l);
        assert!(out.reply().context().is_none());
    }
}

/// Over budget a push drops lines by cut rank, so two kinds never share one
/// (the order would be a coin flip); only the four that were cut before are.
#[test]
fn cut_ranks_are_unique_per_kind() {
    let mut ranks: Vec<u8> = all()
        .iter()
        .filter_map(|l| match policy(&l.kind).cut {
            Cut::Never => None,
            Cut::First(r) | Cut::Last(r) => Some(r),
        })
        .collect();
    let n = ranks.len();
    ranks.sort_unstable();
    ranks.dedup();
    assert_eq!(ranks.len(), n);
    assert_eq!(n, 4, "notice, fixed, ask, cited");
}
