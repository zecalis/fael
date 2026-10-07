//! The noise contract, looped over every `Kind` (PLAN-fael-say-gate): nothing
//! to say is silence, a key-once kind is not said twice in a session, a kind
//! that needs a command is dropped without one, and no session says it all.

use super::changed::read_seen;
use super::say::{Kind, Line, Once, Outbox, Reply, policy};
use super::state::lock_seen;
use std::path::PathBuf;

/// One fixture per kind. `slot` has no `_` arm, so a new `Kind` does not
/// compile until it gets a slot — and `every_kind_has_a_fixture` fails until
/// that slot has a fixture here.
fn all() -> Vec<Line> {
    let line = |kind: Kind, text: &str| Line {
        kind,
        text: text.into(),
    };
    vec![
        line(
            Kind::Row {
                ids: vec!["01ROW".into()],
            },
            "fael mem for a.rs:\n- [01ROW] decision x\n",
        ),
        line(Kind::Brief, "- [01BRIEF] issue y\n"),
        line(
            Kind::Ask {
                ids: vec!["01ASK".into()],
                issue: false,
            },
            "fael: done with one? fael close 01ASK \"<why>\"\n",
        ),
        line(
            Kind::Pointer {
                keys: vec!["auth:login".into()],
            },
            "fael: the prompt names open key(s) auth:login — fael find --key <key>",
        ),
        line(Kind::Bodies, "bodies: fael find <id> (MCP: find id=<id>)\n"),
        line(
            Kind::Cited {
                ids: vec!["01CITED".into()],
            },
            "fael: 01CITED cited in a commit — done? fael close 01CITED \"<why>\"\n",
        ),
        Line::notice("fael: a stashed line\n".into()),
    ]
}

fn slot(k: &Kind) -> usize {
    match k {
        Kind::Row { .. } => 0,
        Kind::Brief => 1,
        Kind::Ask { .. } => 2,
        Kind::Pointer { .. } => 3,
        Kind::Bodies => 4,
        Kind::Cited { .. } => 5,
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
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}

#[test]
fn a_line_without_its_command_is_dropped() {
    for l in all() {
        let Some(command) = policy(&l.kind).command else {
            continue;
        };
        let without = Line {
            text: l.text.replace(command, "fael nowhere"),
            ..l.clone()
        };
        assert_eq!(said(None, without), None, "{:?}", l.kind);
        assert!(said(None, l).is_some());
    }
}

/// A spent mark (the hub peek's) says nothing and is in the seen list the
/// next push opens.
#[test]
fn a_spent_mark_says_nothing_and_is_seen_next_time() {
    let p = seen("s.seen");
    let mut out = Outbox::open(lock_seen(&p));
    out.spend("~peek:x".into());
    assert!(out.reply().context().is_none());
    assert!(Outbox::open(lock_seen(&p)).has("~peek:x"));
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
}

/// A `per_turn` kind speaks once per user turn even on a key not yet spent;
/// the next turn may say it again. No turn marked = no limit.
#[test]
fn a_per_turn_kind_is_said_once_per_turn() {
    for l in all().into_iter().filter(|l| policy(&l.kind).per_turn) {
        let p = seen("s.seen");
        let other = |id: &str| Line {
            kind: Kind::Ask {
                ids: vec![id.into()],
                issue: false,
            },
            ..l.clone()
        };
        let in_turn = |t: &str, l: Line| {
            let mut out = Outbox::open(lock_seen(&p)).in_turn(Some(t.into()));
            out.say(l);
            out.reply().context().map(String::from)
        };
        assert!(in_turn("t1", other("01A")).is_some());
        assert_eq!(
            in_turn("t1", other("01B")),
            None,
            "{:?} twice in a turn",
            l.kind
        );
        assert!(in_turn("t2", other("01B")).is_some());
        // the held-back line spent nothing: 01B is asked once, in t2
        let (_, hinted) = read_seen(&std::fs::read_to_string(&p).unwrap());
        assert!(hinted.contains("01A") && hinted.contains("01B"));
        assert!(said(lock_seen(&p), other("01C")).is_some());
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}

#[test]
fn no_session_says_every_time() {
    for l in all() {
        assert!(said(None, l.clone()).is_some());
        assert!(said(None, l).is_some());
    }
}

/// A shell call's edit side, then its read side: the contexts in that order,
/// the edit side's notice kept.
#[test]
fn two_replies_join_in_order_and_the_first_notice_wins() {
    let reply = |text: &str, notice: Option<&str>| {
        let mut out = Outbox::open(None);
        out.say(Line::notice(text.into()));
        let mut r = out.reply();
        r.notice = notice.map(String::from);
        r
    };
    let r = reply("edit\n", Some("edit side")).and(reply("read\n", Some("read side")));
    assert_eq!(r.context(), Some("edit\nread\n"));
    assert_eq!(r.notice.as_deref(), Some("edit side"));
    let r = reply("", None).and(reply("read\n", Some("read side")));
    assert_eq!(r.context(), Some("read\n"));
    assert_eq!(r.notice.as_deref(), Some("read side"));
    let r = Reply::default().and(Reply::default());
    assert!(r.context().is_none() && r.notice.is_none());
}

/// A seen list written before the say-gate (row ids, `~id`, `~*`, `@id`)
/// still silences what it did, and the `~count:` (dropped) / `~peek:` /
/// `~bodies` keys are never read as a row this session was told.
#[test]
fn an_old_seen_list_reads_beside_the_new_keys() {
    let p = seen("s.seen");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(
        &p,
        "01OLD\n~01ASKED\n~*\n@01OLD\n~count:src/a.rs|file\n~peek:src/a.rs\n~bodies\n",
    )
    .unwrap();
    let (told, hinted) = read_seen(&std::fs::read_to_string(&p).unwrap());
    assert_eq!(told, ["01OLD".to_string()].into());
    for k in [
        "01ASKED",
        "*",
        "count:src/a.rs|file",
        "peek:src/a.rs",
        "bodies",
    ] {
        assert!(hinted.contains(k), "{k} not hinted");
    }
    let line = |kind: Kind, text: &str| Line {
        kind,
        text: text.into(),
    };
    let mut out = Outbox::open(lock_seen(&p));
    for (l, said) in [
        (
            line(
                Kind::Row {
                    ids: vec!["01OLD".into()],
                },
                "- [01OLD] x\n",
            ),
            false,
        ),
        (
            line(
                Kind::Row {
                    ids: vec!["01NEW".into()],
                },
                "- [01NEW] y\n",
            ),
            true,
        ),
        (
            line(
                Kind::Ask {
                    ids: vec!["01ASKED".into()],
                    issue: false,
                },
                "fael close 01ASKED\n",
            ),
            false,
        ),
        (
            line(
                Kind::Ask {
                    ids: vec!["*".into()],
                    issue: false,
                },
                "fael close <id>\n",
            ),
            false,
        ),
        (
            line(
                Kind::Ask {
                    ids: vec!["01OLD".into()],
                    issue: false,
                },
                "fael close 01OLD\n",
            ),
            true,
        ),
        (line(Kind::Bodies, "bodies: fael find <id>\n"), false),
    ] {
        let before = out.fresh(&l.kind);
        assert_eq!(before, said, "{:?}", l.kind);
        out.say(l);
    }
    assert_eq!(
        out.reply().context(),
        Some("- [01NEW] y\nfael close 01OLD\n")
    );
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
}

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
    let lines = pick(&[0, 4, 2, 6]); // row, bodies, hint, notice
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

/// Usage records what was said (chunk 3): a said line names its kind, a
/// dropped one names nothing, and `Reply::and` (a shell call's edit side, then
/// its read side) keeps both sides' entries in order.
#[test]
fn said_names_each_line_said_and_and_keeps_both_sides() {
    let kinds = |r: &Reply| -> Vec<String> {
        let v = serde_json::to_value(r.said()).unwrap();
        let k = |e: &serde_json::Value| e["kind"].as_str().unwrap().to_string();
        v.as_array().unwrap().iter().map(k).collect()
    };
    let (mut both, mut want) = (Reply::default(), vec![]);
    for l in all() {
        let mut out = Outbox::open(None);
        out.say(Line {
            text: String::new(),
            ..l.clone()
        });
        assert!(out.reply().said().is_empty(), "{:?}", l.kind);
        let mut out = Outbox::open(None);
        out.say(l);
        let r = out.reply();
        want.extend(kinds(&r));
        both = both.and(r);
    }
    assert_eq!(kinds(&both), want);
    want.dedup();
    assert_eq!(
        want,
        [
            "row", "brief", "ask", "pointer", "bodies", "cited", "notice"
        ]
    );
}
