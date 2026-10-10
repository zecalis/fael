//! The chunk contract (SPEC §1): transitions, fencing, run end, ready, edit.

use super::super::{Fields, Owner, Ready, Start, Store};
use super::Here;
use rusqlite::params;

pub(crate) const NOW: &str = "2026-10-11T09:00:00Z";

pub(crate) fn here(wt: &str) -> Here<'_> {
    Here {
        worktree: wt,
        branch: Some("b"),
        now: NOW,
    }
}

pub(crate) fn add(s: &mut Store, title: &str, brief: Option<&str>) -> String {
    let f = Fields {
        title: Some(title.into()),
        brief: brief.map(Into::into),
        ..Fields::default()
    };
    s.add("inbox", &f, &[], NOW).unwrap()
}

pub(crate) fn get(s: &Store, sql: &str, uid: &str) -> Option<String> {
    s.conn
        .query_row(sql, [uid], |r| r.get::<_, Option<String>>(0))
        .unwrap()
}

pub(crate) fn state(s: &Store, uid: &str) -> String {
    get(s, "SELECT state FROM chunk WHERE uid = ?1", uid).unwrap()
}

/// Live run rows of a chunk.
pub(crate) fn live(s: &Store, uid: &str) -> i64 {
    s.conn
        .query_row(
            "SELECT COUNT(*) FROM run r JOIN chunk c ON c.id = r.chunk
             WHERE c.uid = ?1 AND r.ended IS NULL",
            [uid],
            |r| r.get(0),
        )
        .unwrap()
}

pub(crate) fn start(s: &mut Store, uid: &str, wt: &str) -> Result<String, String> {
    s.start(uid, &Start::default(), &here(wt)).map(|x| x.run)
}

#[test]
fn add_makes_the_inbox_and_a_draft_never_starts() {
    let mut s = Store::open_in_memory().unwrap();
    let d = add(&mut s, "idea", None);
    let o = add(&mut s, "work", Some("do the work"));
    assert_eq!(
        (state(&s, &d), state(&s, &o)),
        ("draft".into(), "open".into())
    );
    let e = start(&mut s, &d, "/wt").unwrap_err();
    assert!(e.contains("not ready: it is draft"), "{e}");
    let st = s.start(&o, &Start::default(), &here("/wt")).unwrap();
    let text = st.text(&|_| None, &|_| true);
    assert!(
        text.starts_with(&format!("fael run {} ·", st.run)),
        "{text}"
    );
    assert!(text.contains("do the work") && text.contains(&format!("fael chunk done {o}")));
    assert_eq!(s.plans().unwrap()[0].truth, "db");
    let bad = Fields {
        title: Some("x".into()),
        scope: Some(vec!["../etc".into()]),
        ..Fields::default()
    };
    assert!(
        s.add("inbox", &bad, &[], NOW)
            .unwrap_err()
            .contains("scope")
    );
    let f = Fields {
        title: Some("x".into()),
        ..Fields::default()
    };
    assert!(s.add("nope", &f, &[], NOW).unwrap_err().contains("no plan"));
}

#[test]
fn run_end_forced_takeover_and_fencing() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("brief"));
    let r1 = start(&mut s, &c, "/a").unwrap();
    let e = start(&mut s, &c, "/b").unwrap_err();
    assert!(
        e.contains(&format!("held by run {r1}")) && e.contains("in /a"),
        "{e}"
    );
    // run end ends the run, never the state; again is a no-op
    assert_eq!(s.run_end(&r1, NOW).unwrap(), 1);
    assert_eq!(s.run_end(&r1, NOW).unwrap(), 0);
    assert_eq!((state(&s, &c), live(&s, &c)), ("running".into(), 0));
    // an ended run starts again without --force; a reused R is refused
    let reuse = Start {
        run: Some(r1.clone()),
        ..Start::default()
    };
    assert!(
        s.start(&c, &reuse, &here("/b"))
            .unwrap_err()
            .contains("already used")
    );
    let r2 = start(&mut s, &c, "/b").unwrap();
    // --force takes over: /b's run ends, /b is fenced off, /c holds it
    let force = Start {
        force: true,
        client: Some("codex".into()),
        ..Start::default()
    };
    let r3 = s.start(&c, &force, &here("/c")).unwrap().run;
    assert_eq!(live(&s, &c), 1);
    let e = s.wait(&c, "owner", "q?", None, &here("/b")).unwrap_err();
    assert!(
        e.contains(&format!("taken over by run {r3} (codex) in /c")),
        "{e}"
    );
    assert!(s.done(&c, "h", Some(1), None, &here("/b")).is_err());
    // the old shell's cleanup never ends the new start
    assert_eq!(s.run_end(&r2, NOW).unwrap(), 0);
    assert_eq!(live(&s, &c), 1);
    // one start per worktree
    let other = add(&mut s, "other", Some("b"));
    let e = s.after(&c, &other, "why", &here("/b")).unwrap_err();
    assert!(e.contains("taken over"), "after is fenced too: {e}");
    assert_eq!(live(&s, &c), 1, "a fenced after ends no run");
    let e = start(&mut s, &other, "/c").unwrap_err();
    assert!(e.contains(&format!("holds run {r3}")), "{e}");
    assert_eq!(state(&s, &other), "open", "a refused start claims nothing");
    s.done(&c, "shipped", Some(7), None, &here("/c")).unwrap();
    // push pr is the owner's ok: a PR lands in done, no review
    assert_eq!((state(&s, &c), live(&s, &c)), ("done".into(), 0));
    let pr = get(&s, "SELECT CAST(pr AS TEXT) FROM run WHERE start = ?1", &r3);
    assert_eq!(pr.as_deref(), Some("7"));
}

#[test]
fn review_answer_accept() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("brief"));
    start(&mut s, &c, "/a").unwrap();
    s.done(&c, "h", None, None, &here("/a")).unwrap();
    assert!(s.owner(&c, Owner::Unpark, None, NOW).is_err());
    s.owner(&c, Owner::Answer, Some("use the old name"), NOW)
        .unwrap();
    assert_eq!(state(&s, &c), "open");
    let b = s.start(&c, &Start::default(), &here("/a")).unwrap();
    assert_eq!(b.chunks[0].said, vec!["use the old name".to_string()]);
    let mut out = |r: &str| -> Result<String, String> { Ok(format!("/out/{r}")) };
    s.done(&c, "h2", None, Some(&mut out), &here("/a")).unwrap();
    let kept = get(&s, "SELECT out FROM run WHERE start = ?1", &b.run);
    assert_eq!(kept, Some(format!("/out/{}", b.run)));
    // the answer was said once: a third start does not repeat it
    s.owner(&c, Owner::Answer, Some("one more"), NOW).unwrap();
    let b = s.start(&c, &Start::default(), &here("/a")).unwrap();
    assert_eq!(b.chunks[0].said, vec!["one more".to_string()]);
    let mut fail = |_: &str| -> Result<String, String> { Err("no such file".into()) };
    assert!(s.done(&c, "h", None, Some(&mut fail), &here("/a")).is_err());
    assert_eq!(state(&s, &c), "running", "a failed copy changes nothing");
    s.done(&c, "h", None, None, &here("/a")).unwrap();
    s.owner(&c, Owner::Accept, None, NOW).unwrap();
    assert_eq!(state(&s, &c), "done");
    let e = s.owner(&c, Owner::Park, Some("x"), NOW).unwrap_err();
    assert!(e.contains("cannot become parked"), "{e}");
}

#[test]
fn a_data_wait_turns_ready_on_its_date_and_after_blocks() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "post", Some("post it"));
    let m = add(&mut s, "measure", Some("count reach"));
    start(&mut s, &m, "/a").unwrap();
    s.after(&m, &a, "needs the post", &here("/a")).unwrap();
    assert_eq!((state(&s, &m), live(&s, &m)), ("open".into(), 0));
    match s.ready(&m, "2026-10-11").unwrap() {
        Ready::Blocked(u) => assert_eq!(u[0].uid.as_deref(), Some(a.as_str())),
        r => panic!("{r:?}"),
    }
    let e = s.after(&a, &m, "loop", &here("/a")).unwrap_err();
    assert!(e.contains("loop"), "{e}");
    for (c, wt) in [(&a, "/a"), (&m, "/b")] {
        if c == &m {
            s.owner(&a, Owner::Drop, Some("posted by hand"), NOW)
                .unwrap();
        }
        start(&mut s, c, wt).unwrap();
    }
    s.wait(
        &m,
        "data",
        "reach after 2 days",
        Some("2026-10-11"),
        &here("/b"),
    )
    .unwrap();
    assert_eq!(
        s.ready(&m, "2026-10-10").unwrap(),
        Ready::No("it waits on data until 2026-10-11".into())
    );
    assert_eq!(s.ready(&m, "2026-10-11").unwrap(), Ready::Yes);
    start(&mut s, &m, "/b").unwrap();
    s.wait(&m, "owner", "which channel?", None, &here("/b"))
        .unwrap();
    assert!(matches!(s.ready(&m, "2030-01-01").unwrap(), Ready::No(_)));
    assert!(s.wait(&m, "boss", "x", None, &here("/b")).is_err());
}

#[test]
fn edit_clears_the_approval_and_never_the_state() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("brief"));
    s.conn
        .execute(
            "UPDATE chunk SET approved_at = ?1, approved_model = 'opus' WHERE uid = ?2",
            params![NOW, c],
        )
        .unwrap();
    let f = Fields {
        brief: Some("a new brief".into()),
        scope: Some(vec!["fael-core/src/plan/".into(), "docs/a.md".into()]),
        ..Fields::default()
    };
    s.edit(&c, &f, NOW).unwrap();
    assert_eq!(
        get(&s, "SELECT approved_at FROM chunk WHERE uid = ?1", &c),
        None
    );
    assert_eq!(
        get(&s, "SELECT scope FROM chunk WHERE uid = ?1", &c).as_deref(),
        Some("fael-core/src/plan/\ndocs/a.md")
    );
    assert_eq!(state(&s, &c), "open");
    let ev = get(
        &s,
        "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC LIMIT 1",
        &c,
    );
    assert_eq!(
        ev.as_deref(),
        Some("edited brief, scope · approval cleared")
    );
    assert!(s.edit(&c, &Fields::default(), NOW).is_err());
}

#[test]
fn the_stop_hook_stamps_only_its_own_live_run() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    let b = add(&mut s, "b", Some("b"));
    let plan = s.plans().unwrap()[0].id;
    let first = |s: &Store| s.first_ready(plan, "2026-10-11").unwrap().map(|x| x.0);
    assert_eq!(first(&s).as_deref(), Some(a.as_str()));
    let r = start(&mut s, &a, "/w").unwrap();
    assert_eq!(
        first(&s).as_deref(),
        Some(b.as_str()),
        "a running chunk is not next"
    );
    assert_eq!(
        s.held("/w").unwrap(),
        vec![(a.clone(), "a".into(), r.clone())]
    );
    assert_eq!(s.seen("/w", "s1", NOW).unwrap(), 1, "fills session");
    assert_eq!(s.seen("/w", "s2", NOW).unwrap(), 0, "another session's run");
    assert_eq!(s.seen("/x", "s1", NOW).unwrap(), 0, "another worktree");
    s.run_end(&r, NOW).unwrap();
    assert_eq!(s.seen("/w", "s1", NOW).unwrap(), 0, "never an ended run");
    assert!(s.held("/w").unwrap().is_empty());
}
