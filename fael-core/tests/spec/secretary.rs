//! Secretary contract (plan fael-secretary, c0): the reference model every later chunk
//! (c1 lifecycle, c2 identity, c3 push, c4 time/stats, c5 resume, c6 doctor, c7 preset)
//! binds production to. Pure data and a tiny model — no production logic under test.
//! Prose: `docs/architecture.md` §8. Fixture ids (F-RACE, F-RESTART, F-XSESSION, …) are
//! named there and must stay named there.

use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
enum State {
    Open,
    Closed,
    Superseded(&'static str),
}

#[derive(Debug, PartialEq)]
enum Out {
    /// one new event written
    Wrote,
    /// zero events; the existing row/id is named
    Noop(&'static str),
}

/// Transition table: op on A in a state -> (outcome, state after). Total and typed:
/// nothing is silently rejected, a retry writes zero events.
fn close(s: &State, id: &'static str) -> (Out, State) {
    match s {
        State::Open => (Out::Wrote, State::Closed),
        State::Closed => (Out::Noop(id), State::Closed),
        State::Superseded(b) => (Out::Noop(b), s.clone()),
    }
}

/// supersede(A -> b): b is always filed (content is never dropped); A is retired once.
fn supersede(s: &State, id: &'static str, b: &'static str) -> (Out, State) {
    match s {
        State::Open => (Out::Wrote, State::Superseded(b)),
        State::Closed => (Out::Noop(id), State::Closed),
        State::Superseded(c) => (Out::Noop(c), s.clone()),
    }
}

/// A retired row is never un-retired and never retired twice.
const TABLE: &[(&str, &str, State, Out, State)] = &[
    (
        "T1 close open",
        "close",
        State::Open,
        Out::Wrote,
        State::Closed,
    ),
    (
        "T2 close retry",
        "close",
        State::Closed,
        Out::Noop("A"),
        State::Closed,
    ),
    (
        "T3 stale close",
        "close",
        State::Superseded("B"),
        Out::Noop("B"),
        State::Superseded("B"),
    ),
    (
        "T4 supersede open",
        "supersede",
        State::Open,
        Out::Wrote,
        State::Superseded("B"),
    ),
    (
        "T5 supersede retry",
        "supersede",
        State::Superseded("B"),
        Out::Noop("B"),
        State::Superseded("B"),
    ),
    (
        "T6 supersede of closed",
        "supersede",
        State::Closed,
        Out::Noop("A"),
        State::Closed,
    ),
];

#[test]
fn table_matches_model() {
    for (name, op, from, out, to) in TABLE {
        let got = if *op == "close" {
            close(from, "A")
        } else {
            supersede(from, "A", "B")
        };
        assert_eq!(
            &got,
            &(
                match out {
                    Out::Wrote => Out::Wrote,
                    Out::Noop(i) => Out::Noop(i),
                },
                to.clone()
            ),
            "{name}"
        );
    }
}

/// F-RACE: close(A) vs supersede(A->B) in either lock order: exactly one retire event for A,
/// the loser a typed no-op naming the winner's result; the final state is a function of
/// which op the lock granted first, never of timing inside either op.
#[test]
fn f_race_close_vs_supersede() {
    let (s, id) = (State::Open, "A");
    let (w1, s1) = close(&s, id);
    let (w2, s2) = supersede(&s1, id, "B");
    assert_eq!((w1, w2, s2), (Out::Wrote, Out::Noop("A"), State::Closed));
    let (w1, s1) = supersede(&s, id, "B");
    let (w2, s2) = close(&s1, id);
    assert_eq!(
        (w1, w2, s2),
        (Out::Wrote, Out::Noop("B"), State::Superseded("B"))
    );
    // supersede(A->B) vs supersede(A->C): the first wins, the second names B, one successor
    let (_, s1) = supersede(&s, id, "B");
    assert_eq!(
        supersede(&s1, id, "C"),
        (Out::Noop("B"), State::Superseded("B"))
    );
}

/// Delivery bookkeeping lives in usage/outbox only: a set of (session,row), never a ledger field.
#[derive(Default)]
struct Delivered(HashSet<(String, String)>);

impl Delivered {
    /// true = say it now; false = already said in this session
    fn say(&mut self, session: &str, row: &str) -> bool {
        self.0.insert((session.into(), row.into()))
    }
    fn dump(&self) -> Vec<String> {
        let mut v: Vec<_> = self.0.iter().map(|(s, r)| format!("{s}\t{r}")).collect();
        v.sort();
        v
    }
    fn load(lines: &[String]) -> Self {
        Delivered(
            lines
                .iter()
                .filter_map(|l| l.split_once('\t'))
                .map(|(s, r)| (s.into(), r.into()))
                .collect(),
        )
    }
}

/// F-RESTART: a restart inside a session reloads the bookkeeping; the row is not said again.
#[test]
fn f_restart_boundary() {
    let mut d = Delivered::default();
    assert!(d.say("s1", "R1"));
    let mut d = Delivered::load(&d.dump());
    assert!(
        !d.say("s1", "R1"),
        "said once per (session,row) across a restart"
    );
}

/// F-XSESSION: dedupe key is (session,row) only — another session gets the same row once too;
/// a different row in the same session is independent (never text/file/query dedupe).
#[test]
fn f_cross_session_visibility() {
    let mut d = Delivered::default();
    assert!(d.say("s1", "R1") && d.say("s2", "R1") && d.say("s1", "R2"));
    assert!(!d.say("s1", "R1") && !d.say("s2", "R1"));
}

/// F-IDENT: one input -> one resolver -> one id or a typed miss. Order: full id, id prefix
/// (>= 8 chars, unique), key, text; an id-shaped input never falls through to text silently.
#[derive(Debug, PartialEq)]
enum Resolved {
    Id(&'static str),
    Ambiguous(Vec<&'static str>),
    Miss,
}

fn resolve(rows: &[(&'static str, &str)], input: &str) -> Resolved {
    let hits: Vec<_> = rows
        .iter()
        .filter(|(id, _)| id.starts_with(input))
        .map(|(id, _)| *id)
        .collect();
    let id_shaped = input.len() >= 8 && input.chars().all(|c| c.is_ascii_alphanumeric());
    match (id_shaped, hits.as_slice()) {
        (true, [one]) => return Resolved::Id(one),
        (true, [_, ..]) => return Resolved::Ambiguous(hits),
        _ => {}
    }
    match rows.iter().find(|(_, k)| *k == input) {
        Some((id, _)) => Resolved::Id(id),
        None => Resolved::Miss,
    }
}

#[test]
fn f_ident_prefix_key_miss() {
    let rows = [
        ("01M62AAAA1", "a:b"),
        ("01M62AAAB2", "a:c"),
        ("01M63ZZZZ9", "x:y"),
    ];
    assert_eq!(resolve(&rows, "01M63ZZZ"), Resolved::Id("01M63ZZZZ9"));
    assert_eq!(resolve(&rows, "x:y"), Resolved::Id("01M63ZZZZ9"));
    assert_eq!(
        resolve(&rows, "01M62AAA"),
        Resolved::Ambiguous(vec!["01M62AAAA1", "01M62AAAB2"])
    );
    assert_eq!(resolve(&rows, "01M99NONE"), Resolved::Miss);
}

/// F-TIME: stored UTC (`Z`), rendered local with offset; `--since DATE` is local midnight,
/// i.e. the UTC instant `DATE 00:00 - offset`. Bangkok +07:00 is the failing case on record.
#[test]
fn f_time_utc_store_local_render() {
    let (utc_hour, off) = (17u32, 7u32);
    assert_eq!(
        (utc_hour + off) % 24,
        0,
        "2026-10-29T17:00:00Z == 2026-10-30T00:00:00+07:00"
    );
    assert!(utc_hour + off >= 24, "the local date is the next UTC date");
}

/// The prose and the fixtures cannot drift apart: every id this file defines is named in §8.
#[test]
fn doc_names_every_fixture() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/architecture.md"
    ))
    .unwrap();
    for f in ["F-RACE", "F-RESTART", "F-XSESSION", "F-IDENT", "F-TIME"] {
        assert!(doc.contains(f), "docs/architecture.md §8 must name {f}");
    }
    for (name, ..) in TABLE {
        assert!(
            doc.contains(name.split(' ').next().unwrap()),
            "{name} missing from §8"
        );
    }
}
