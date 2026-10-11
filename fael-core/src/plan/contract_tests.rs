//! Contracts (PLAN-fael-parallel §2): every transition of the lifecycle, who may speak, loops.

use super::super::chunk::tests::{NOW, add, here, start};
use super::super::{Here, Store};
use super::Reply;

/// Two chunks, each running in its own worktree: `(store, a, b)`; a will ask b.
fn lanes() -> (Store, String, String) {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    let b = add(&mut s, "b", Some("b"));
    start(&mut s, &a, "/a").unwrap();
    start(&mut s, &b, "/b").unwrap();
    (s, a, b)
}

fn ask(s: &mut Store, wt: &str, to: &str) -> String {
    s.ask(
        to,
        "a new column",
        "fn col() -> u8",
        Some("cargo test"),
        &here(wt),
    )
    .unwrap()
}

fn state(s: &Store, id: &str) -> String {
    s.conn
        .query_row("SELECT state FROM contract WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
}

fn column(s: &Store, id: &str, col: &str) -> Option<String> {
    s.conn
        .query_row(
            &format!("SELECT CAST({col} AS TEXT) FROM contract WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

fn counter<'a>(shape: &'a str) -> Reply<'a> {
    Reply::Counter { shape, why: "why" }
}

#[test]
fn ask_keeps_the_askers_run_and_writes_the_event() {
    let (mut s, a, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    assert_eq!(state(&s, &id), "open");
    assert_eq!(super::super::chunk::tests::live(&s, &a), 1);
    assert_eq!(super::super::chunk::tests::state(&s, &a), "running");
    let (to, text): (String, String) = s
        .conn
        .query_row(
            "SELECT to_state, text FROM event WHERE chunk_uid = 'contract:' || ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(to, "open");
    assert!(
        text.contains("a new column") && text.contains("fn col()"),
        "{text}"
    );
    // a's view: out, b's view: in; the check is kept, never run
    let out = s.asks(&a).unwrap();
    let inn = s.asks(&b).unwrap();
    assert_eq!((out[0].dir, out[0].chunk.as_str()), ("out", b.as_str()));
    assert_eq!((inn[0].dir, inn[0].chunk.as_str()), ("in", a.as_str()));
    assert_eq!(inn[0].check.as_deref(), Some("cargo test"));
}

#[test]
fn ask_is_refused_for_itself_nobody_a_finished_chunk_and_missing_words() {
    let (mut s, a, b) = lanes();
    let e = s.ask(&a, "w", "r", None, &here("/a")).unwrap_err();
    assert!(e.contains("cannot ask itself"), "{e}");
    let e = s.ask(&b, "w", "r", None, &here("/nowhere")).unwrap_err();
    assert!(e.contains("live run"), "{e}");
    let e = s.ask(&b, "w", " ", None, &here("/a")).unwrap_err();
    assert!(e.contains("--returns"), "{e}");
    s.done(&b, "ok", Some(1), None, &here("/b")).unwrap();
    let e = s.ask(&b, "w", "r", None, &here("/a")).unwrap_err();
    assert!(e.contains("is done"), "{e}");
}

#[test]
fn an_ask_that_closes_a_loop_is_refused() {
    let (mut s, a, b) = lanes();
    let c = add(&mut s, "c", Some("c"));
    start(&mut s, &c, "/c").unwrap();
    ask(&mut s, "/a", &b);
    ask(&mut s, "/b", &c);
    let e = s.ask(&a, "w", "r", None, &here("/c")).unwrap_err();
    assert!(e.contains("closes a loop"), "{e}");
    // two chunks asking each other is the shortest loop
    let e = s.ask(&a, "w", "r", None, &here("/b")).unwrap_err();
    assert!(e.contains("closes a loop"), "{e}");
}

#[test]
fn a_withdrawn_ask_no_longer_holds_a_loop_open() {
    let (mut s, a, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    s.withdraw(&id, &here("/a")).unwrap();
    assert_eq!(state(&s, &id), "withdrawn");
    ask(&mut s, "/b", &a);
}

#[test]
fn the_holder_agrees() {
    let (mut s, _, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    // the asker cannot answer its own ask
    let e = s.reply(&id, Reply::Agree(None), &here("/a")).unwrap_err();
    assert!(e.contains("answers it"), "{e}");
    assert_eq!(state(&s, &id), "open");
    assert_eq!(
        s.reply(&id, Reply::Agree(Some("done by noon")), &here("/b"))
            .unwrap(),
        "agreed"
    );
    assert_eq!(
        column(&s, &id, "returns").as_deref(),
        Some("fn col() -> u8")
    );
    let last: String = s
        .conn
        .query_row(
            "SELECT text FROM event WHERE chunk_uid = 'contract:' || ?1 ORDER BY id DESC",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(last, "done by noon");
    // an agreed contract takes no further reply
    let e = s.reply(&id, Reply::Agree(None), &here("/b")).unwrap_err();
    assert!(e.contains("is agreed"), "{e}");
}

#[test]
fn the_holder_counters_and_the_asker_agrees_to_the_counter_shape() {
    let (mut s, _, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    assert_eq!(
        s.reply(&id, counter("fn col() -> u16"), &here("/b"))
            .unwrap(),
        "countered"
    );
    assert_eq!(
        (
            column(&s, &id, "rounds").unwrap().as_str(),
            column(&s, &id, "counter").unwrap().as_str()
        ),
        ("1", "fn col() -> u16")
    );
    // the holder does not answer its own counter
    let e = s.reply(&id, Reply::Agree(None), &here("/b")).unwrap_err();
    assert!(e.contains("answers it"), "{e}");
    assert_eq!(
        s.reply(&id, Reply::Agree(None), &here("/a")).unwrap(),
        "agreed"
    );
    assert_eq!(
        column(&s, &id, "returns").as_deref(),
        Some("fn col() -> u16")
    );
}

#[test]
fn a_second_counter_goes_to_the_owner_who_settles_it() {
    let (mut s, _, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    s.reply(&id, counter("u16"), &here("/b")).unwrap();
    assert_eq!(s.reply(&id, counter("u32"), &here("/a")).unwrap(), "owner");
    assert_eq!(column(&s, &id, "rounds").as_deref(), Some("2"));
    let e = s.reply(&id, Reply::Agree(None), &here("/b")).unwrap_err();
    assert!(
        e.contains("waits on the owner") && e.contains("chunk answer"),
        "{e}"
    );
    assert!(s.is_contract(&id[..8]));
    s.answer_contract(&id[..8], "u64", NOW).unwrap();
    assert_eq!(state(&s, &id), "agreed");
    assert_eq!(column(&s, &id, "returns").as_deref(), Some("u64"));
    let by: String = s
        .conn
        .query_row(
            "SELECT by FROM event WHERE chunk_uid = 'contract:' || ?1 ORDER BY id DESC",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(by, "owner");
    let e = s.answer_contract(&id, "x", NOW).unwrap_err();
    assert!(e.contains("is agreed"), "{e}");
}

#[test]
fn withdraw_is_the_askers_while_nothing_is_agreed() {
    let (mut s, _, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    let e = s.withdraw(&id, &here("/b")).unwrap_err();
    assert!(e.contains("answers it"), "{e}");
    s.reply(&id, counter("u16"), &here("/b")).unwrap();
    s.withdraw(&id, &here("/a")).unwrap();
    assert_eq!(state(&s, &id), "withdrawn");
    let e = s.reply(&id, Reply::Agree(None), &here("/a")).unwrap_err();
    assert!(e.contains("is withdrawn"), "{e}");
    let id = ask(&mut s, "/a", &b);
    s.reply(&id, Reply::Agree(None), &here("/b")).unwrap();
    let e = s.withdraw(&id, &here("/a")).unwrap_err();
    assert!(e.contains("not yet agreed"), "{e}");
}

#[test]
fn a_reply_comes_only_from_a_live_run_and_a_shell_may_name_its_start() {
    let (mut s, _, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    // b's worktree took over by someone else, or ended: no live run, no reply
    s.run_end(
        &s.conn
            .query_row(
                "SELECT start FROM run r JOIN chunk c ON c.id = r.chunk WHERE c.uid = ?1",
                [&b],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
        None,
        NOW,
    )
    .unwrap();
    let e = s.reply(&id, Reply::Agree(None), &here("/b")).unwrap_err();
    assert!(e.contains("answers it"), "{e}");
    let (mut s, a, b) = lanes();
    let run_a: String = s
        .conn
        .query_row(
            "SELECT start FROM run r JOIN chunk c ON c.id = r.chunk WHERE c.uid = ?1",
            [&a],
            |r| r.get(0),
        )
        .unwrap();
    let by_run = Here {
        run: Some(&run_a),
        ..here("/elsewhere")
    };
    let id = s.ask(&b, "w", "r", None, &by_run).unwrap();
    assert_eq!(state(&s, &id), "open");
}

#[test]
fn an_unknown_contract_is_named() {
    let (mut s, _, _) = lanes();
    let e = s
        .reply("NOPE", Reply::Agree(None), &here("/a"))
        .unwrap_err();
    assert!(e.contains("no contract NOPE"), "{e}");
}
