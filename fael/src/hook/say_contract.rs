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
            },
            "fael: done with one? fael close 01ASK \"<why>\"\n",
        ),
        line(
            Kind::Pointer {
                keys: vec!["auth:login".into()],
            },
            "fael: the prompt names open key(s) auth:login — fael find --key <key>",
        ),
        line(
            Kind::Count {
                keys: vec!["src/hub.rs|file".into()],
            },
            "… +6 more about this file — fael find --files src/hub.rs\n",
        ),
        line(Kind::Bodies, "bodies: fael find <id> (MCP: find id=<id>)\n"),
        Line::notice("fael: a stashed line\n".into()),
    ]
}

fn slot(k: &Kind) -> usize {
    match k {
        Kind::Row { .. } => 0,
        Kind::Brief => 1,
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

/// A count line this session was told stays silent; a new kind of cut on
/// the same file set is still said.
#[test]
fn a_new_count_line_is_said_beside_a_told_one() {
    let p = seen("s.seen");
    let count = |keys: &[&str]| Line {
        kind: Kind::Count {
            keys: keys.iter().map(|k| k.to_string()).collect(),
        },
        text: "… +1 more — fael find --files x\n".into(),
    };
    assert!(said(lock_seen(&p), count(&["x|file"])).is_some());
    let mut out = Outbox::open(lock_seen(&p));
    assert!(!out.fresh(&count(&["x|file"]).kind));
    assert!(out.fresh(&count(&["x|dir:src/"]).kind));
    out.say(count(&["x|file"]));
    assert!(out.reply().context().is_none());
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
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
/// still silences what it did, and the new `~count:` / `~bodies` keys are
/// never read as a row this session was told.
#[test]
fn an_old_seen_list_reads_beside_the_new_keys() {
    let p = seen("s.seen");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(
        &p,
        "01OLD\n~01ASKED\n~*\n@01OLD\n~count:src/a.rs|file\n~bodies\n",
    )
    .unwrap();
    let (told, hinted) = read_seen(&std::fs::read_to_string(&p).unwrap());
    assert_eq!(told, ["01OLD".to_string()].into());
    for k in ["01ASKED", "*", "count:src/a.rs|file", "bodies"] {
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
                },
                "fael close 01ASKED\n",
            ),
            false,
        ),
        (
            line(
                Kind::Ask {
                    ids: vec!["*".into()],
                },
                "fael close <id>\n",
            ),
            false,
        ),
        (
            line(
                Kind::Ask {
                    ids: vec!["01OLD".into()],
                },
                "fael close 01OLD\n",
            ),
            true,
        ),
        (
            line(
                Kind::Count {
                    keys: vec!["src/a.rs|file".into()],
                },
                "… +1 — fael find --files src/a.rs\n",
            ),
            false,
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
