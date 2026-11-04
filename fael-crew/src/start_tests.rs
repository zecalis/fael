//! `chunk start`: one winner of a race, a pair all-or-none, per-member commands end only
//! that member's run (SPEC §4 test matrix).

use super::super::chunk::tests::{NOW, add, get, here, live, start, state};
use super::super::{Fields, Here, Owner, Start, Store};
use rusqlite::params;

fn pair(s: &Store, a: &str, b: &str) {
    s.conn
        .execute(
            "INSERT INTO edge(src, dst, kind) SELECT x.id, y.id, 'pair'
             FROM chunk x, chunk y WHERE x.uid = ?1 AND y.uid = ?2",
            params![a, b],
        )
        .unwrap();
}

#[test]
fn a_pair_member_command_ends_only_its_own_run() {
    for cmd in ["wait", "after", "done", "drop", "park"] {
        for first in [true, false] {
            let mut s = Store::open_in_memory().unwrap();
            let a = add(&mut s, "a", Some("a"));
            let b = add(&mut s, "b", Some("b"));
            let other = add(&mut s, "other", Some("o"));
            pair(&s, &a, &b);
            let r = start(&mut s, &a, "/w").unwrap();
            assert_eq!((live(&s, &a), live(&s, &b)), (1, 1), "one R, both claimed");
            let (m, rest) = if first { (&a, &b) } else { (&b, &a) };
            let h = here("/w");
            match cmd {
                "wait" => s.wait(m, "owner", "q", None, &[], &h),
                "after" => s.after(m, &other, "why", &h),
                "done" => s.done(m, "h", Some(3), None, &h),
                "drop" => s.owner(m, Owner::Drop, Some("no"), NOW),
                _ => s.owner(m, Owner::Park, Some("later"), NOW),
            }
            .unwrap();
            let at = format!(
                "{cmd} on the {} member",
                if first { "first" } else { "second" }
            );
            assert_eq!((live(&s, m), live(&s, rest)), (0, 1), "{at}");
            assert_eq!(state(&s, rest), "running", "{at}");
            assert_eq!(
                s.run_end(&r, None, NOW).unwrap(),
                1,
                "{at}: run end ends the rest"
            );
            assert_eq!(state(&s, rest), "running", "{at}: run end keeps the state");
        }
    }
}

#[test]
fn a_pair_starts_whole_or_not_at_all() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    let b = add(&mut s, "b", Some("b"));
    pair(&s, &a, &b);
    s.owner(&b, Owner::Park, Some("later"), NOW).unwrap();
    let e = start(&mut s, &a, "/w").unwrap_err();
    assert!(
        e.contains(&format!("chunk {b} (paired with {a}) is not ready")),
        "{e}"
    );
    assert_eq!((state(&s, &a), live(&s, &a)), ("open".into(), 0));
    s.owner(&b, Owner::Unpark, None, NOW).unwrap();
    let st = s.start(&b, &Default::default(), &here("/w")).unwrap();
    assert_eq!(st.chunks.len(), 2);
    assert!(st.text().contains("2 paired chunks"));
}

#[test]
fn a_blocked_chunk_starts_only_with_force() {
    let mut s = Store::open_in_memory().unwrap();
    let first = add(&mut s, "first", Some("f"));
    let shipped = add(&mut s, "shipped", Some("s"));
    s.conn
        .execute(
            "INSERT INTO edge(src, dst, kind) SELECT x.id, y.id, 'after'
             FROM chunk x, chunk y WHERE x.uid = ?1 AND y.uid = ?2",
            params![shipped, first],
        )
        .unwrap();
    let e = start(&mut s, &shipped, "/w").unwrap_err();
    assert!(e.contains("is blocked") && e.contains("--force"), "{e}");
    // bare --force is rejected: it names --because (issue 01M6CZ0K)
    let bare = Start {
        force: true,
        ..Start::default()
    };
    let e = s.start(&shipped, &bare, &here("/w")).unwrap_err();
    assert!(e.contains("--because"), "{e}");
    let o = Start {
        force: true,
        because: Some("will rebase after first lands".into()),
        ..Start::default()
    };
    let st = s.start(&shipped, &o, &here("/w")).unwrap();
    assert_eq!(st.forced.len(), 1, "{:?}", st.forced);
    assert!(
        st.forced[0].contains("--force past after"),
        "{:?}",
        st.forced
    );
    assert!(st.forced[0].contains("will rebase"), "{:?}", st.forced);
    s.done(&shipped, "landed in #383", Some(383), None, &here("/w"))
        .unwrap();
    assert_eq!(state(&s, &shipped), "done");
    let q = "SELECT text FROM event WHERE chunk_uid = ?1 AND to_state = 'running'";
    let ev = get(&s, q, &shipped).unwrap();
    assert!(ev.contains(&first), "{ev}");
    assert!(ev.contains("because will rebase"), "{ev}");
    let reason: Option<String> = s
        .conn
        .query_row(
            "SELECT force_reason FROM run WHERE chunk = (SELECT id FROM chunk WHERE uid = ?1)",
            [&shipped],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reason.as_deref(), Some("will rebase after first lands"));
}

#[test]
fn two_starts_of_one_chunk_one_wins() {
    // a race shows on some runs only: 30 rounds make one test run catch it
    for _ in 0..30 {
        let dir = std::env::temp_dir().join(format!("fael-start-{}", fael_core::ulid()));
        let path = dir.join("crew.db");
        let c = add(&mut Store::open(&path).unwrap(), "c", Some("brief"));
        let racers: Vec<_> = ["/a", "/b"]
            .into_iter()
            .map(|wt| {
                let (p, c) = (path.clone(), c.clone());
                std::thread::spawn(move || start(&mut Store::open(&p).unwrap(), &c, wt))
            })
            .collect();
        let res: Vec<_> = racers.into_iter().map(|t| t.join().unwrap()).collect();
        let won: Vec<_> = res.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(won.len(), 1, "{res:?}");
        let lost = res.iter().find_map(|r| r.as_ref().err()).unwrap();
        assert!(lost.contains(&format!("held by run {}", won[0])), "{lost}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn a_no_worktree_start_takes_no_slot_and_its_shell_finishes_it() {
    let mut s = Store::open_in_memory().unwrap();
    let img = add(&mut s, "hero image", Some("draw it"));
    let code = add(&mut s, "code", Some("c"));
    let o = Start {
        run: Some("R1".into()),
        client: Some("image".into()),
        model: Some("flux".into()),
        no_worktree: true,
        ..Start::default()
    };
    s.start(&img, &o, &here("/main")).unwrap();
    let run = "SELECT r.model || ' ' || COALESCE(r.worktree, '-') || ' ' || COALESCE(r.branch, '-')
               FROM run r JOIN chunk c ON c.id = r.chunk WHERE c.uid = ?1";
    assert_eq!(get(&s, run, &img).as_deref(), Some("flux - -"));
    // the shell's worktree is still free for an agent
    start(&mut s, &code, "/main").unwrap();
    let by = |r: &'static str| Here {
        run: Some(r),
        ..here("/elsewhere")
    };
    let e = s.done(&img, "h", None, None, &by("R0")).unwrap_err();
    assert!(e.contains("latest run is R1 (image), not R0"), "{e}");
    s.done(&img, "drawn", None, None, &by("R1")).unwrap();
    assert_eq!((state(&s, &img), live(&s, &img)), ("review".into(), 0));
}

#[test]
fn size_is_s_m_or_l_and_an_edit_clears_the_approval() {
    let mut s = Store::open_in_memory().unwrap();
    let f = |size: &str| Fields {
        title: Some("t".into()),
        size: Some(size.into()),
        ..Fields::default()
    };
    let c = s.add("inbox", &f("S"), &[], NOW).unwrap();
    let e = s.add("inbox", &f("XL"), &[], NOW).unwrap_err();
    assert!(e.contains("--size XL — S, M or L"), "{e}");
    s.approve(&c, Some((Some("deepseek"), Some("opencode"))), NOW)
        .unwrap();
    s.edit(&c, &f("L"), NOW).unwrap();
    let q = "SELECT size || ' ' || COALESCE(approved_at, 'unapproved') FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &c).as_deref(), Some("L unapproved"));
}

/// Issue 01M4RRD1K: done's diff replaces the declared scope; a path the scope rule refuses
/// is left out, and no diff keeps what was declared.
#[test]
fn claim_takes_the_real_diff_as_scope() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("b"));
    s.conn
        .execute("UPDATE chunk SET scope = 'x/' WHERE uid = ?1", [&c])
        .unwrap();
    let scope = |s: &Store| get(s, "SELECT scope FROM chunk WHERE uid = ?1", &c);
    s.claim(&c, &[]).unwrap();
    assert_eq!(scope(&s).as_deref(), Some("x/"));
    let diff = ["a.rs", "app/[id]/page.tsx", "docs/b.md"].map(String::from);
    s.claim(&c, &diff).unwrap();
    assert_eq!(scope(&s).as_deref(), Some("a.rs\ndocs/b.md"));
}

/// Issue 01M4TPHMR: the next brief names an after-chunk's files only once its done claimed
/// a diff — a declared scope that was never claimed is no file it touched.
#[test]
fn only_a_claimed_diff_reaches_the_next_brief_as_files_touched() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("b"));
    let f = Fields {
        title: Some("next".into()),
        brief: Some("b".into()),
        ..Fields::default()
    };
    let b = s.add("inbox", &f, std::slice::from_ref(&a), NOW).unwrap();
    s.conn
        .execute("UPDATE chunk SET scope = 'a/' WHERE uid = ?1", [&a])
        .unwrap();
    start(&mut s, &a, "/a").unwrap();
    s.done(&a, "h", Some(1), None, &here("/a")).unwrap();
    let files = |s: &mut Store| {
        let st = s.start(&b, &Start::default(), &here("/b")).unwrap();
        s.run_end(&st.run, None, NOW).unwrap();
        st.chunks[0].after[0].2.clone()
    };
    assert_eq!(files(&mut s), "", "declared a/, never claimed");
    s.claim(&a, &["z/q.rs".into()]).unwrap();
    assert_eq!(files(&mut s), "z/q.rs");
}

/// A review chunk's brief names the PR of the chunk it is after — no git-log hunt.
#[test]
fn the_after_line_names_the_pr_of_the_chunk_it_follows() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("b"));
    let f = Fields {
        title: Some("review a".into()),
        brief: Some("b".into()),
        ..Fields::default()
    };
    let b = s.add("inbox", &f, std::slice::from_ref(&a), NOW).unwrap();
    start(&mut s, &a, "/a").unwrap();
    s.done(&a, "h", Some(1273), None, &here("/a")).unwrap();
    let st = s.start(&b, &Start::default(), &here("/b")).unwrap();
    assert_eq!(st.chunks[0].after[0].0, "a (#1273)");
}

fn scoped(s: &mut Store, title: &str, scope: &str) -> String {
    let c = add(s, title, Some(title));
    s.conn
        .execute(
            "UPDATE chunk SET scope = ?2 WHERE uid = ?1",
            params![c, scope],
        )
        .unwrap();
    c
}

#[test]
fn a_scope_overlap_with_a_running_chunk_never_refuses() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs\ndocs/");
    start(&mut s, &a, "/w1").unwrap();
    // same file, another worktree: information (the brief's avoid card), no refusal
    let st = s.start(&b, &Start::default(), &here("/w2")).unwrap();
    assert_eq!(state(&s, &b), "running");
    assert!(st.forced.is_empty(), "{:?}", st.forced);
    assert_eq!(live(&s, &a), 1, "the other run is left alone");
    let avoid = &st.chunks[0].avoid;
    assert!(
        avoid.len() == 1 && avoid[0].uid == a && avoid[0].do_cmd.contains("fael chunk status"),
        "{avoid:?}"
    );
    assert!(
        avoid[0].do_cmd.contains(&format!("fael chunk note {a} ")),
        "{avoid:?}"
    );
}

/// The one refusal left (issue 01M6KHWF): a live run from another known session in this worktree.
#[test]
fn a_live_run_of_another_session_in_the_same_worktree_refuses() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/a.rs");
    let b = scoped(&mut s, "b", "docs/");
    sess(&mut s, &a, "/w1", "s1").unwrap();
    let e = sess(&mut s, &b, "/w1", "s2").unwrap_err();
    assert!(
        e.contains(&a) && e.contains("another worktree") && e.contains("--force --because"),
        "{e}"
    );
    assert_eq!(state(&s, &b), "open");
    assert_eq!(live(&s, &a), 1);
    // bare --force still names the receipt
    let bare = Start {
        force: true,
        session: Some("s2".into()),
        ..Start::default()
    };
    let e = s.start(&b, &bare, &here("/w1")).unwrap_err();
    assert!(e.contains("--because"), "{e}");
    let o = Start {
        because: Some("a is done editing".into()),
        ..bare
    };
    let st = s.start(&b, &o, &here("/w1")).unwrap();
    assert_eq!(st.forced.len(), 1, "{:?}", st.forced);
    assert!(st.forced[0].contains("a is done editing") && st.forced[0].contains(&a));
    assert_eq!(live(&s, &a), 0, "the forced start took the seat");
    assert_eq!(st.orphaned.len(), 1);
}

/// The caller's own session left its work: its old run ends silently, as before.
#[test]
fn a_run_of_the_callers_own_session_ends_without_refusal() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/a.rs");
    let b = scoped(&mut s, "b", "docs/");
    sess(&mut s, &a, "/w1", "s1").unwrap();
    let st = s
        .start(
            &b,
            &Start {
                session: Some("s1".into()),
                ..Start::default()
            },
            &here("/w1"),
        )
        .unwrap();
    assert!(st.forced.is_empty());
    assert_eq!((live(&s, &a), live(&s, &b)), (0, 1));
    assert_eq!(st.orphaned.len(), 1);
}

/// An unknown session (either side) never refuses: only two known, different sessions do.
#[test]
fn an_unknown_session_never_refuses() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/a.rs");
    let b = scoped(&mut s, "b", "docs/");
    let c = scoped(&mut s, "c", "lib/");
    let d = scoped(&mut s, "d", "bin/");
    start(&mut s, &a, "/w1").unwrap(); // run without a session
    sess(&mut s, &b, "/w1", "s2").unwrap(); // caller known, other unknown
    assert_eq!((live(&s, &a), live(&s, &b)), (0, 1));
    start(&mut s, &c, "/w1").unwrap(); // caller unknown, other known
    assert_eq!((live(&s, &b), live(&s, &c)), (0, 1));
    sess(&mut s, &d, "/w1", "").unwrap(); // empty counts as unknown
}

/// A stalled run there is an agent that left: the start ends it, no refusal, as before.
#[test]
fn a_stalled_run_in_the_same_worktree_is_ended_without_force() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/a.rs");
    let b = scoped(&mut s, "b", "docs/");
    start(&mut s, &a, "/w1").unwrap();
    let o = Start {
        stale: "2999-01-01T00:00:00Z".into(),
        ..Start::default()
    };
    let st = s.start(&b, &o, &here("/w1")).unwrap();
    assert!(st.forced.is_empty());
    assert_eq!(live(&s, &a), 0);
    assert_eq!(st.orphaned.len(), 1);
}

#[test]
fn a_pair_partner_or_an_idle_chunk_is_no_overlap() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/");
    let b = scoped(&mut s, "b", "src/x.rs");
    let c = scoped(&mut s, "c", "src/y.rs");
    pair(&s, &a, &b);
    let r = start(&mut s, &a, "/w1").unwrap();
    assert_eq!(state(&s, &b), "running", "pair starts together");
    s.run_end(&r, None, NOW).unwrap();
    start(&mut s, &c, "/w2").unwrap();
}

#[test]
fn a_mixed_model_is_recorded_on_the_run_from_approve_or_start() {
    let q = "SELECT r.model FROM run r JOIN chunk c ON c.id = r.chunk WHERE c.uid = ?1";
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    s.approve(&a, Some((Some("mixed"), None)), NOW).unwrap();
    start(&mut s, &a, "/a").unwrap();
    assert_eq!(get(&s, q, &a).as_deref(), Some("mixed"));
    let b = add(&mut s, "b", Some("b"));
    let o = Start {
        model: Some("mixed".into()),
        ..Start::default()
    };
    s.start(&b, &o, &here("/b")).unwrap();
    assert_eq!(get(&s, q, &b).as_deref(), Some("mixed"));
}

fn sess(s: &mut Store, uid: &str, wt: &str, session: &str) -> Result<String, String> {
    let o = Start {
        session: Some(session.into()),
        ..Start::default()
    };
    s.start(uid, &o, &here(wt)).map(|x| x.run)
}

#[test]
fn a_live_run_in_the_same_worktree_is_taken_over_without_force() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("brief"));
    let r1 = sess(&mut s, &c, "/a", "s1").unwrap();
    // another worktree still needs --force
    let e = sess(&mut s, &c, "/b", "s2").unwrap_err();
    assert!(
        e.contains(&format!("held by run {r1}")) && e.contains("--force"),
        "{e}"
    );
    assert_eq!(live(&s, &c), 1);
    // a new session in the same worktree: the seat is its own
    let r2 = sess(&mut s, &c, "/a", "s3").unwrap();
    assert_ne!(r1, r2);
    assert_eq!(live(&s, &c), 1);
    let ev = get(
        &s,
        "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC LIMIT 1",
        &c,
    )
    .unwrap();
    assert!(
        ev.contains("taken over from run") && ev.contains(&r2),
        "{ev}"
    );
}

#[test]
fn start_reports_the_chunk_it_orphaned_in_the_worktree() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    let b = add(&mut s, "b", Some("b"));
    // a stalled run is the agent that left: the start ends it (a live one refuses)
    let o = Start {
        stale: "2999-01-01T00:00:00Z".into(),
        ..Start::default()
    };
    let go = |s: &mut Store, u: &str| s.start(u, &o, &here("/w")).unwrap();
    assert!(go(&mut s, &a).orphaned.is_empty());
    let again = go(&mut s, &a);
    assert!(
        again.orphaned.is_empty(),
        "same chunk restarted: nothing orphaned"
    );
    let st = go(&mut s, &b);
    assert_eq!(st.orphaned, vec![(a.clone(), "a".to_string())]);
    assert_eq!(state(&s, &a), "running", "informs only: state untouched");
}

/// c5: same-session retry reuses the live R (lifecycle truth: the run row),
/// never mints a new one; a retry without a session is a takeover, not a resume.
#[test]
fn start_resume_same_session_reuses_live_r() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("brief"));
    let o = Start {
        session: Some("s1".into()),
        ..Start::default()
    };
    let first = s.start(&c, &o, &here("/w")).unwrap();
    assert!(!first.resumed);
    let second = s.start(&c, &o, &here("/w")).unwrap();
    assert!(second.resumed, "same session retries the live run");
    assert_eq!(first.run, second.run, "one live R per session");
    assert_eq!(live(&s, &c), 1, "no new run row");
    assert!(
        second.render_short().contains("(resumed)")
            && second.render_short().len() < second.render(false, false).len(),
        "short unless --full"
    );
}

/// c5: `--on data` shape-checks before record — no `--until` rejects and writes
/// nothing, so the hint prints only on valid shape.
#[test]
fn start_resume_wait_data_needs_until_before_record() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("brief"));
    start(&mut s, &c, "/w").unwrap();
    let before: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))
        .unwrap();
    let e = s
        .wait(&c, "data", "hits", None, &[], &here("/w"))
        .unwrap_err();
    assert!(e.contains("--until"), "{e}");
    let after: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM event", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after, "invalid shape writes nothing");
    assert_eq!(state(&s, &c), "running");
}

#[test]
fn a_start_on_an_owner_wait_names_who_unblocks_it() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    start(&mut s, &a, "/w").unwrap();
    s.wait(&a, "owner", "q?", None, &[], &here("/w")).unwrap();
    let e = start(&mut s, &a, "/w2").unwrap_err();
    assert!(e.contains("it waits on owner"), "{e}");
    assert!(
        e.contains(&format!("fael chunk answer {a} \"<text>\"")) && e.contains("the owner's"),
        "{e}"
    );
}

/// A stalled holder of the same chunk is no refusal (fael:01M6CZ9G41): starting
/// it again from another worktree takes it over with an automatic receipt —
/// no --force, no --because.
#[test]
fn stalled_holder_takeover_receipt() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    start(&mut s, &a, "/w1").unwrap();
    let o = Start {
        stale: "2999-01-01T00:00:00Z".into(),
        ..Start::default()
    };
    s.start(&a, &o, &here("/w2")).unwrap();
    let ev = get(
        &s,
        "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC LIMIT 1",
        &a,
    )
    .unwrap();
    assert!(
        ev.contains("taken over") && ev.contains(&format!("partner {a} stalled since")),
        "{ev}"
    );
    // a live holder still refuses without --force
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    start(&mut s, &a, "/w1").unwrap();
    let e = s.start(&a, &Start::default(), &here("/w2")).unwrap_err();
    assert!(e.contains("is held by run") && e.contains("--force"), "{e}");
}

/// A stack is its own seat: the caller's worktree keeps its live run (a pair would free
/// it) and the run row records mode stack + partner, so the next brief knows the PR
/// order. Exact overlap with the partner is no refusal once the tip check passed.
#[test]
fn a_stack_start_keeps_its_own_seat_and_records_stack() {
    use super::super::Joint;
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs");
    let c = scoped(&mut s, "c", "other/");
    let ra = start(&mut s, &a, "/w1").unwrap();
    start(&mut s, &c, "/w2").unwrap();
    let o = Start {
        with: vec![a.clone()],
        joint: Some(Joint::Stack),
        stack_contains: vec![true],
        stale: "2999-01-01T00:00:00Z".into(), // c in /w2 is stalled: its seat is freed
        ..Start::default()
    };
    let st = s.start(&b, &o, &here("/w2")).unwrap();
    assert_ne!(st.run, ra, "stack keeps its own R, not the partner's");
    assert_eq!(
        live(&s, &c),
        0,
        "caller seat /w2 freed for the stack's own run"
    );
    assert_eq!(live(&s, &a), 1, "partner run stays live");
    assert_eq!(live(&s, &b), 1, "stack runs");
    let joint: Option<String> = s
        .conn
        .query_row(
            "SELECT r.joint FROM run r JOIN chunk k ON k.id = r.chunk WHERE k.uid = ?1",
            [&b],
            |r| r.get(0),
        )
        .unwrap();
    let partner: Option<String> = s
        .conn
        .query_row(
            "SELECT r.partner FROM run r JOIN chunk k ON k.id = r.chunk WHERE k.uid = ?1",
            [&b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        (joint.as_deref(), partner.as_deref()),
        (Some("stack"), Some(a.as_str()))
    );
    let ev = get(
        &s,
        "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC LIMIT 1",
        &b,
    )
    .unwrap();
    assert!(
        ev.contains(&format!("joint stack with {a}")),
        "start event records the stack: {ev}"
    );
}

/// A stack without an ancestor read per partner is a programming error, never a start.
#[test]
fn a_stack_without_an_ancestor_read_is_rejected() {
    use super::super::Joint;
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs");
    start(&mut s, &a, "/w1").unwrap();
    let o = Start {
        with: vec![a.clone()],
        joint: Some(Joint::Stack),
        stale: super::super::chunk::tests::NOW.into(),
        ..Start::default()
    };
    let e = s.start(&b, &o, &here("/w2")).unwrap_err();
    assert!(e.contains("stack needs ancestor"), "{e}");
}
