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
        run: None,
    }
}

/// Same as `here`, on another branch (a different branch is a different work).
pub(crate) fn here_on<'a>(wt: &'a str, branch: &'a str) -> Here<'a> {
    Here {
        branch: Some(branch),
        ..here(wt)
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
    let text = st.text();
    assert!(
        text.starts_with(&format!("fael run {} ·", st.run)),
        "{text}"
    );
    assert!(text.contains("do the work") && text.contains(&format!("fael chunk done {o}")));
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
    assert_eq!(s.run_end(&r1, None, NOW).unwrap(), 1);
    assert_eq!(s.run_end(&r1, None, NOW).unwrap(), 0);
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
    let e = s
        .wait(&c, "owner", "q?", None, &[], &here_on("/b", "x"))
        .unwrap_err();
    assert!(
        e.contains(&format!(
            "run {r3} (codex) was started in /c, not here (/b)"
        )),
        "{e}"
    );
    assert!(
        e.contains(&format!("try the same command with --run {r3}")),
        "the reject names the flag: {e}"
    );
    assert!(s.done(&c, "h", Some(1), None, &here_on("/b", "x")).is_err());
    // the run's own branch from another worktree is the same work: passes
    s.touch(&c, &["src/z.rs".to_string()], &here("/z")).unwrap();
    // the old shell's cleanup never ends the new start
    assert_eq!(s.run_end(&r2, None, NOW).unwrap(), 0);
    assert_eq!(live(&s, &c), 1);
    // one start per worktree
    let other = add(&mut s, "other", Some("b"));
    let e = s.after(&c, &other, "why", &here_on("/b", "x")).unwrap_err();
    assert!(e.contains("was started in"), "after is fenced too: {e}");
    assert_eq!(live(&s, &c), 1, "a fenced after ends no run");
    // one worktree is one seat: another chunk's live run there ends, the start goes through
    start(&mut s, &other, "/c").unwrap();
    assert_eq!((live(&s, &c), live(&s, &other)), (0, 1));
    assert_eq!(
        state(&s, &c),
        "running",
        "the ended run keeps its chunk's state"
    );
    let r3 = start(&mut s, &c, "/c").unwrap();
    s.done(&c, "shipped", Some(7), None, &here("/c")).unwrap();
    // push pr is the owner's ok: a PR lands in done, no review
    assert_eq!((state(&s, &c), live(&s, &c)), ("done".into(), 0));
    let pr = get(&s, "SELECT CAST(pr AS TEXT) FROM run WHERE start = ?1", &r3);
    assert_eq!(pr.as_deref(), Some("7"));
    // done ended the run first: the launcher's run end still records what R cost, only R
    assert_eq!(s.run_end(&r3, Some(1.25), NOW).unwrap(), 0);
    let cost = |r: &str| get(&s, "SELECT CAST(cost AS TEXT) FROM run WHERE start = ?1", r);
    assert_eq!((cost(&r3), cost(&r2)), (Some("1.25".into()), None));
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
        &[],
        &here("/b"),
    )
    .unwrap();
    assert_eq!(
        s.ready(&m, "2026-10-10").unwrap(),
        Ready::No("it waits on data until 2026-10-11".into())
    );
    assert_eq!(s.ready(&m, "2026-10-11").unwrap(), Ready::Yes);
    start(&mut s, &m, "/b").unwrap();
    s.wait(&m, "owner", "which channel?", None, &[], &here("/b"))
        .unwrap();
    assert!(matches!(s.ready(&m, "2030-01-01").unwrap(), Ready::No(_)));
    assert!(s.wait(&m, "boss", "x", None, &[], &here("/b")).is_err());
    let ab = ["A".to_string(), "B".to_string()];
    let e = s.wait(&m, "data", "x", None, &ab, &here("/b")).unwrap_err();
    assert_eq!(e, "rejected: --option needs --on owner; try --on owner");
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
fn edit_brief_opens_a_draft() {
    let mut s = Store::open_in_memory().unwrap();
    let d = add(&mut s, "idea", None);
    let title = Fields {
        title: Some("idea, named".into()),
        ..Fields::default()
    };
    s.edit(&d, &title, NOW).unwrap();
    assert_eq!(state(&s, &d), "draft", "no brief, still a draft");
    let brief = Fields {
        brief: Some("do it".into()),
        ..Fields::default()
    };
    s.edit(&d, &brief, NOW).unwrap();
    assert_eq!(state(&s, &d), "open");
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
    s.run_end(&r, None, NOW).unwrap();
    assert_eq!(s.seen("/w", "s1", NOW).unwrap(), 0, "never an ended run");
    assert!(s.held("/w").unwrap().is_empty());
}

#[test]
fn notes_and_the_handoff_before_reach_the_next_brief() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("b"));
    let f = Fields {
        title: Some("next".into()),
        brief: Some("b".into()),
        ..Fields::default()
    };
    let b = s.add("inbox", &f, std::slice::from_ref(&a), NOW).unwrap();
    start(&mut s, &a, "/a").unwrap();
    // from another worktree, on a running chunk: no fence, no state change, not a start
    s.note(&b, "use the v2 fixture", NOW).unwrap();
    s.note(&a, "too late for a's brief", NOW).unwrap();
    assert_eq!(state(&s, &a), "running");
    s.done(
        &a,
        "board ships; app reads FAEL_BIN",
        Some(1),
        None,
        &here("/a"),
    )
    .unwrap();
    assert!(s.note(&a, "x", NOW).unwrap_err().contains("is done"));
    let st = s.start(&b, &Start::default(), &here("/b")).unwrap();
    let br = &st.chunks[0];
    assert_eq!(br.notes, ["use the v2 fixture"]);
    assert_eq!(
        br.after,
        [(
            "a (#1)".to_string(),
            "board ships; app reads FAEL_BIN".to_string(),
            String::new()
        )]
    );
    s.note(&b, "mid-run word", NOW).unwrap();
    s.done(&b, "h", None, None, &here("/b")).unwrap();
    s.owner(&b, Owner::Answer, Some("redo"), NOW).unwrap();
    // the second start: only what came since the first, the mid-run note is not a start
    let br = &s.start(&b, &Start::default(), &here("/b")).unwrap().chunks[0];
    assert_eq!(
        (br.notes.as_slice(), br.said.as_slice()),
        (&["mid-run word".to_string()][..], &["redo".to_string()][..])
    );
    let t = s.board("2026-10-11", NOW).unwrap();
    let hb = t.chunks.iter().find(|c| c.uid == b).unwrap();
    assert_eq!(
        hb.handoff.as_ref().unwrap().text,
        "h",
        "a note is never the hand-off"
    );
}

#[test]
fn accept_takes_only_a_review_chunk() {
    let mut s = Store::open_in_memory().unwrap();
    let events = |s: &Store| -> i64 {
        s.conn
            .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))
            .unwrap()
    };
    let [o, r, w, p] = ["o", "r", "w", "p"].map(|t| add(&mut s, t, Some("brief")));
    start(&mut s, &r, "/r").unwrap();
    start(&mut s, &w, "/w").unwrap();
    s.wait(&w, "owner", "which?", None, &[], &here("/w"))
        .unwrap();
    s.owner(&p, Owner::Park, Some("later"), NOW).unwrap();
    for (c, st) in [
        (&o, "open"),
        (&r, "running"),
        (&w, "waiting"),
        (&p, "parked"),
    ] {
        let before = events(&s);
        let e = s.owner(c, Owner::Accept, None, NOW).unwrap_err();
        assert_eq!(
            e,
            format!(
                "rejected: chunk {c} is {st} — `accept` takes a chunk in review; try `fael chunk show {c}`"
            )
        );
        assert_eq!((state(&s, c), events(&s)), (st.to_string(), before));
    }
    s.done(&r, "h", None, None, &here("/r")).unwrap();
    s.owner(&r, Owner::Accept, None, NOW).unwrap();
    assert_eq!(state(&s, &r), "done");
}

/// `chunk done` with no uid finds the chunk by the worktree's live run: held there only.
#[test]
fn held_names_the_chunk_of_this_worktree_only() {
    let mut s = Store::open_in_memory().unwrap();
    let u = add(&mut s, "t", Some("do it"));
    start(&mut s, &u, "/w").unwrap();
    let held = s.held("/w").unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].0, u);
    assert!(s.held("/elsewhere").unwrap().is_empty());
}
