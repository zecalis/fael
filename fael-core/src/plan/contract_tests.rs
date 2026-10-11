//! Contracts (PLAN-fael-parallel §2): every transition of the lifecycle, who may speak,
//! what is refused at once, loops.

use super::super::chunk::tests::{NOW, add, here, live, start, state as chunk_state};
use super::super::{Here, Store};
use super::Reply;

/// Two chunks, each running in its own worktree: `(store, a, b)`; a started first, so a's
/// claim is the older.
fn lanes() -> (Store, String, String) {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    let b = add(&mut s, "b", Some("b"));
    start(&mut s, &a, "/a").unwrap();
    start(&mut s, &b, "/b").unwrap();
    (s, a, b)
}

/// An ask that is not refused at once.
fn ask(s: &mut Store, wt: &str, to: &str) -> String {
    let (id, refused) = s
        .ask(
            to,
            "a new column",
            "fn col() -> u8",
            Some("cargo test"),
            &here(wt),
        )
        .unwrap();
    assert_eq!(refused, None);
    id
}

/// An ask the store refuses at once: `(id, reason)`.
fn refused(s: &mut Store, wt: &str, to: &str) -> (String, String) {
    let (id, why) = s.ask(to, "w", "r", None, &here(wt)).unwrap();
    (id, why.expect("refused at once"))
}

fn col(s: &Store, id: &str, col: &str) -> Option<String> {
    s.conn
        .query_row(
            &format!("SELECT {col} FROM contract WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap()
}

fn state(s: &Store, id: &str) -> String {
    col(s, id, "state").unwrap()
}

fn agree(note: Option<&str>) -> Reply<'_> {
    Reply::Agree {
        note,
        returns: None,
    }
}

fn last_event(s: &Store, id: &str) -> (String, String, String) {
    s.conn
        .query_row(
            "SELECT to_state, text, by FROM event WHERE chunk_uid = 'contract:' || ?1
             ORDER BY id DESC",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

#[test]
fn ask_keeps_the_askers_run_and_writes_the_event() {
    let (mut s, a, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    assert_eq!(state(&s, &id), "open");
    assert_eq!((live(&s, &a), chunk_state(&s, &a).as_str()), (1, "running"));
    let (to, text, by) = last_event(&s, &id);
    assert_eq!((to.as_str(), by.as_str()), ("open", "agent"));
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
fn ask_is_rejected_for_itself_nobody_and_missing_words() {
    let (mut s, a, b) = lanes();
    let e = s.ask(&a, "w", "r", None, &here("/a")).unwrap_err();
    assert!(e.contains("cannot ask itself"), "{e}");
    let e = s.ask(&b, "w", "r", None, &here("/nowhere")).unwrap_err();
    assert!(e.contains("live run"), "{e}");
    let e = s.ask(&b, "w", " ", None, &here("/a")).unwrap_err();
    assert!(e.contains("--returns"), "{e}");
    let n: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM contract", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn a_holder_that_is_not_live_refuses_at_once_and_the_asker_goes_on() {
    let (mut s, a, b) = lanes();
    let idle = add(&mut s, "idle", Some("never started"));
    let parked = add(&mut s, "parked", Some("p"));
    s.owner(&parked, super::super::Owner::Park, None, NOW)
        .unwrap();
    let parked_run = add(&mut s, "pr", Some("p"));
    start(&mut s, &parked_run, "/p").unwrap();
    s.owner(&parked_run, super::super::Owner::Park, None, NOW)
        .unwrap();
    // review (a file out, no PR), done (a PR), ended (the run is over, the chunk still running)
    let review = add(&mut s, "review", Some("r"));
    start(&mut s, &review, "/r").unwrap();
    s.done(&review, "out", None, None, &here("/r")).unwrap();
    let done = add(&mut s, "done", Some("d"));
    start(&mut s, &done, "/d").unwrap();
    s.done(&done, "pr", Some(1), None, &here("/d")).unwrap();
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
    for (to, word) in [
        (&idle, "is open"),
        (&parked, "is parked"),
        (&parked_run, "is parked"),
        (&review, "is review"),
        (&done, "is done"),
        (&b, "run ended"),
    ] {
        let (id, why) = refused(&mut s, "/a", to);
        assert!(why.contains(word), "{to}: {why}");
        assert_eq!(state(&s, &id), "refused");
        assert_eq!(col(&s, &id, "reason"), Some(why.clone()));
        let (to_state, text, _) = last_event(&s, &id);
        assert!(to_state == "refused" && text.contains(&why), "{text}");
    }
    assert_eq!((live(&s, &a), chunk_state(&s, &a).as_str()), (1, "running"));
    // a refused ask is over: it answers nothing more and cannot be withdrawn
    let (id, _) = refused(&mut s, "/a", &idle);
    let e = s.reply(&id, agree(None), &here("/b")).unwrap_err();
    assert!(e.contains("is refused"), "{e}");
    assert!(
        s.withdraw(&id, &here("/a"))
            .unwrap_err()
            .contains("is refused")
    );
}

#[test]
fn the_younger_ask_closing_a_loop_is_refused_at_once() {
    let (mut s, a, b) = lanes();
    let first = ask(&mut s, "/a", &b);
    // b started later: its ask back to a is the younger claim's
    let (id, why) = refused(&mut s, "/b", &a);
    assert!(why.contains("older claim wins"), "{why}");
    assert_eq!(
        (state(&s, &id).as_str(), state(&s, &first).as_str()),
        ("refused", "open")
    );
    // a longer loop: a → b → c, and c (the youngest) asks a
    let c = add(&mut s, "c", Some("c"));
    start(&mut s, &c, "/c").unwrap();
    ask(&mut s, "/b", &c);
    let (id, _) = refused(&mut s, "/c", &a);
    assert_eq!(state(&s, &id), "refused");
}

#[test]
fn the_older_ask_closing_a_loop_refuses_the_younger_open_ask_on_it() {
    let (mut s, a, b) = lanes();
    let young = ask(&mut s, "/b", &a);
    // a is the older claim: its ask stands and b's open ask to a gives way
    let old = ask(&mut s, "/a", &b);
    assert_eq!(
        (state(&s, &old).as_str(), state(&s, &young).as_str()),
        ("open", "refused")
    );
    assert!(
        col(&s, &young, "reason")
            .unwrap()
            .contains("older claim wins")
    );
    let (to, _, _) = last_event(&s, &young);
    assert_eq!(to, "refused");
}

#[test]
fn an_agreed_ask_on_the_loop_is_not_given_up() {
    let (mut s, a, b) = lanes();
    let young = ask(&mut s, "/b", &a);
    s.reply(&young, agree(None), &here("/a")).unwrap();
    let (id, why) = refused(&mut s, "/a", &b);
    assert!(why.contains("older claim wins"), "{why}");
    assert_eq!(
        (state(&s, &id).as_str(), state(&s, &young).as_str()),
        ("refused", "agreed")
    );
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
fn the_holder_agrees_and_may_name_the_shape_it_will_deliver() {
    let (mut s, _, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    // the asker cannot answer its own ask
    let e = s.reply(&id, agree(None), &here("/a")).unwrap_err();
    assert!(e.contains("answers it"), "{e}");
    assert_eq!(state(&s, &id), "open");
    assert_eq!(
        s.reply(&id, agree(Some("by noon")), &here("/b")).unwrap(),
        "agreed"
    );
    assert_eq!(col(&s, &id, "returns").as_deref(), Some("fn col() -> u8"));
    assert_eq!(last_event(&s, &id).1, "by noon");
    // its answer is final
    let e = s.reply(&id, Reply::Refuse("no"), &here("/b")).unwrap_err();
    assert!(e.contains("is agreed") && e.contains("final"), "{e}");
    let id = ask(&mut s, "/a", &b);
    let r = Reply::Agree {
        note: None,
        returns: Some("fn col() -> u16"),
    };
    s.reply(&id, r, &here("/b")).unwrap();
    assert_eq!(col(&s, &id, "returns").as_deref(), Some("fn col() -> u16"));
    assert_eq!(last_event(&s, &id).1, "fn col() -> u16");
}

#[test]
fn the_holder_refuses_with_a_reason_and_it_is_final() {
    let (mut s, _, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    assert_eq!(
        s.reply(&id, Reply::Refuse("schema is frozen"), &here("/b"))
            .unwrap(),
        "refused"
    );
    assert_eq!(col(&s, &id, "reason").as_deref(), Some("schema is frozen"));
    assert_eq!(col(&s, &id, "returns").as_deref(), Some("fn col() -> u8"));
    assert_eq!(last_event(&s, &id).1, "schema is frozen");
    let e = s.reply(&id, agree(None), &here("/b")).unwrap_err();
    assert!(e.contains("is refused") && e.contains("final"), "{e}");
    assert_eq!(
        s.asks(&b).unwrap()[0].reason.as_deref(),
        Some("schema is frozen")
    );
}

#[test]
fn withdraw_is_the_askers_while_the_ask_is_open() {
    let (mut s, _, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    let e = s.withdraw(&id, &here("/b")).unwrap_err();
    assert!(e.contains("withdraws it"), "{e}");
    s.withdraw(&id, &here("/a")).unwrap();
    assert_eq!(state(&s, &id), "withdrawn");
    let e = s.reply(&id, agree(None), &here("/b")).unwrap_err();
    assert!(e.contains("is withdrawn"), "{e}");
    let id = ask(&mut s, "/a", &b);
    s.reply(&id, agree(None), &here("/b")).unwrap();
    let e = s.withdraw(&id, &here("/a")).unwrap_err();
    assert!(e.contains("only an open ask"), "{e}");
}

#[test]
fn a_reply_comes_only_from_a_live_run_and_a_shell_may_name_its_start() {
    let (mut s, a, b) = lanes();
    let id = ask(&mut s, "/a", &b);
    let start_of = |s: &Store, uid: &str| -> String {
        s.conn
            .query_row(
                "SELECT start FROM run r JOIN chunk c ON c.id = r.chunk WHERE c.uid = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap()
    };
    // b's run is over: nobody answers from its worktree any more
    s.run_end(&start_of(&s, &b), None, NOW).unwrap();
    let e = s.reply(&id, agree(None), &here("/b")).unwrap_err();
    assert!(e.contains("answers it"), "{e}");
    let run_a = start_of(&s, &a);
    let by_run = Here {
        run: Some(&run_a),
        ..here("/elsewhere")
    };
    // a's start, named from a shell elsewhere, asks the live chunk it names
    let c = add(&mut s, "c", Some("c"));
    start(&mut s, &c, "/c").unwrap();
    let (id, refused) = s.ask(&c, "w", "r", None, &by_run).unwrap();
    assert_eq!((refused, state(&s, &id).as_str()), (None, "open"));
}

#[test]
fn an_unknown_contract_is_named() {
    let (mut s, _, _) = lanes();
    let e = s.reply("NOPE", agree(None), &here("/a")).unwrap_err();
    assert!(e.contains("no contract NOPE"), "{e}");
}

#[test]
fn an_ask_of_a_finished_chunk_closes_no_loop() {
    let (mut s, a, b) = lanes();
    let c = add(&mut s, "c", Some("c"));
    start(&mut s, &c, "/c").unwrap();
    // c asked a and was agreed, b asked c; then c finished: a → b closes no loop
    let c_a = ask(&mut s, "/c", &a);
    s.reply(&c_a, agree(None), &here("/a")).unwrap();
    let b_c = ask(&mut s, "/b", &c);
    s.done(&c, "pr", Some(1), None, &here("/c")).unwrap();
    ask(&mut s, "/a", &b);
    // and no ask on the finished chunk's account is given up
    assert_eq!(state(&s, &b_c), "open");
}

#[test]
fn the_youngest_open_ask_in_the_middle_of_a_longer_loop_gives_way() {
    let (mut s, a, b) = lanes();
    let c = add(&mut s, "c", Some("c"));
    start(&mut s, &c, "/c").unwrap();
    let b_c = ask(&mut s, "/b", &c);
    let c_a = ask(&mut s, "/c", &a);
    // a is the oldest claim: a → b stands, c's ask (the youngest asker) gives way
    let a_b = ask(&mut s, "/a", &b);
    let st = |id: &str| state(&s, id);
    assert_eq!(
        (st(&b_c).as_str(), st(&c_a).as_str(), st(&a_b).as_str()),
        ("open", "refused", "open")
    );
}

#[test]
fn a_contract_table_from_an_earlier_build_gains_the_reason_column() {
    let dir = std::env::temp_dir().join(format!("fael-plans-{}", crate::ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plans.db");
    drop(Store::open(&path).unwrap());
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch(
        "DROP TABLE contract;
         CREATE TABLE contract(id TEXT PRIMARY KEY, from_chunk INTEGER NOT NULL,
           to_chunk INTEGER NOT NULL, want TEXT NOT NULL, returns TEXT NOT NULL, check_cmd TEXT,
           counter TEXT, state TEXT NOT NULL, rounds INTEGER NOT NULL DEFAULT 0);",
    )
    .unwrap();
    drop(c);
    let mut s = Store::open(&path).unwrap();
    let a = add(&mut s, "a", Some("a"));
    let b = add(&mut s, "b", Some("b"));
    start(&mut s, &a, "/a").unwrap();
    start(&mut s, &b, "/b").unwrap();
    let (id, _) = s.ask(&b, "w", "r", None, &here("/a")).unwrap();
    s.reply(&id, Reply::Refuse("no"), &here("/b")).unwrap();
    assert_eq!(col(&s, &id, "reason").as_deref(), Some("no"));
    let _ = std::fs::remove_dir_all(dir);
}
