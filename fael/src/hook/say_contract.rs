//! The noise contract, looped over every `Kind` (PLAN-fael-say-gate): nothing
//! to say is silence, a key-once kind is not said twice in a session, a kind
//! that needs a command is dropped without one, and no session says it all.

use super::say::{Kind, Line, Once, Outbox, policy};
use super::state::lock_seen;
use std::path::PathBuf;

/// One fixture per kind. `slot` has no `_` arm, so a new `Kind` does not
/// compile until it gets a slot — and `every_kind_has_a_fixture` fails until
/// that slot has a fixture here.
fn all() -> Vec<Line> {
    let line = |kind: Kind, text: &str, action: Option<&str>| Line {
        kind,
        text: text.into(),
        action: action.map(String::from),
    };
    vec![
        line(
            Kind::Row {
                ids: vec!["01ROW".into()],
            },
            "fael mem for a.rs:\n- [01ROW] decision x\n",
            None,
        ),
        line(
            Kind::Brief {
                ids: vec!["01BRIEF".into()],
            },
            "- [01BRIEF] issue y\n",
            None,
        ),
        line(
            Kind::Ask {
                ids: vec!["01ASK".into()],
            },
            "fael: done with one? fael close 01ASK \"<why>\"\n",
            Some("fael close"),
        ),
        line(
            Kind::Pointer {
                keys: vec!["auth:login".into()],
            },
            "fael: the prompt names open key(s) auth:login — fael find --key <key>",
            Some("fael find --key <key>"),
        ),
        line(
            Kind::Count {
                files: "src/hub.rs".into(),
            },
            "… +6 more about this file — fael find --files src/hub.rs\n",
            Some("fael find --files"),
        ),
        line(
            Kind::Bodies,
            "bodies: fael find <id> (MCP: find id=<id>)\n",
            Some("fael find <id>"),
        ),
        Line::notice("fael: a stashed line\n".into()),
    ]
}

fn slot(k: &Kind) -> usize {
    match k {
        Kind::Row { .. } => 0,
        Kind::Brief { .. } => 1,
        Kind::Ask { .. } => 2,
        Kind::Pointer { .. } => 3,
        Kind::Count { .. } => 4,
        Kind::Bodies => 5,
        Kind::Notice => 6,
    }
}

/// A fresh seen list of its own.
fn seen(name: &str) -> PathBuf {
    std::env::temp_dir()
        .join(format!("fael-say-{}", crate::core::ulid()))
        .join(name)
}

fn said(file: Option<std::fs::File>, l: Line) -> Option<String> {
    let mut out = Outbox::open(file);
    out.say(l);
    out.reply().context().map(String::from)
}

#[test]
fn every_kind_has_a_fixture() {
    let slots: Vec<usize> = all().iter().map(|l| slot(&l.kind)).collect();
    assert_eq!(slots, (0..slots.len()).collect::<Vec<_>>());
}

#[test]
fn nothing_to_say_is_silent() {
    assert!(Outbox::open(None).reply().context().is_none());
    for l in all() {
        let empty = Line {
            text: String::new(),
            ..l
        };
        assert_eq!(said(None, empty), None);
    }
}

#[test]
fn a_key_once_kind_is_not_said_twice_in_a_session() {
    for l in all() {
        let p = seen("s.seen");
        let first = said(lock_seen(&p), l.clone());
        assert_eq!(first.as_deref(), Some(l.text.as_str()), "{:?}", l.kind);
        let again = said(lock_seen(&p), l.clone());
        match policy(&l.kind).once {
            Once::Key => assert_eq!(again, None, "{:?} said twice", l.kind),
            // its event fires once (session start, a stash taken off disk);
            // it spends no key another kind could be silenced by
            Once::Event => assert_eq!(std::fs::read_to_string(&p).unwrap(), ""),
        }
    }
}

#[test]
fn a_line_without_its_command_is_dropped() {
    for l in all().into_iter().filter(|l| policy(&l.kind).needs_action) {
        let none = Line {
            action: None,
            ..l.clone()
        };
        assert_eq!(said(None, none), None, "{:?}", l.kind);
        let elsewhere = Line {
            action: Some("fael nowhere".into()),
            ..l.clone()
        };
        assert_eq!(said(None, elsewhere), None, "{:?}", l.kind);
        assert!(said(None, l).is_some());
    }
}

#[test]
fn no_session_says_every_time() {
    for l in all() {
        assert!(said(None, l.clone()).is_some());
        assert!(said(None, l).is_some());
    }
}

/// PLAN-fael-say-gate chunk 6: rows, bodies and counts keep their say; over
/// the budget the stashed notice goes first, then the edit hint, and a cut
/// line spends no key (a later push may say it).
#[test]
fn over_the_budget_the_notice_goes_then_the_hint_never_the_count() {
    let pick = |slots: &[usize]| -> Vec<Line> {
        all()
            .into_iter()
            .filter(|l| slots.contains(&slot(&l.kind)))
            .collect()
    };
    let cost = |ls: &[Line]| -> usize { ls.iter().map(|l| crate::core::est_tokens(&l.text)).sum() };
    let lines = pick(&[0, 4, 2, 6]); // row, count, hint, notice
    let keep = cost(&pick(&[0, 4]));
    let run = |budget: usize| {
        let p = seen("s.seen");
        let mut out = Outbox::open(lock_seen(&p));
        out.say_within(budget, lines.clone());
        let said = out.reply().context().unwrap_or("").to_string();
        (said, std::fs::read_to_string(&p).unwrap())
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
    assert!(
        said.contains("fael find --files") && said.contains("01ROW"),
        "{said}"
    );
    // the cut hint kept its key
    assert!(
        keys.contains("~01ASK") && !keys_cut.contains("~01ASK"),
        "{keys} / {keys_cut}"
    );
    // no budget at all still says the rows and the count
    assert!(run(0).0.contains("fael find --files"));
}
