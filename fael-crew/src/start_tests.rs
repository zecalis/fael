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
    let o = Start {
        force: true,
        ..Start::default()
    };
    s.start(&shipped, &o, &here("/w")).unwrap();
    s.done(&shipped, "landed in #383", Some(383), None, &here("/w"))
        .unwrap();
    assert_eq!(state(&s, &shipped), "done");
    let q = "SELECT text FROM event WHERE chunk_uid = ?1 AND to_state = 'running'";
    assert!(get(&s, q, &shipped).unwrap().contains(&first));
}

#[test]
fn two_starts_of_one_chunk_one_wins() {
    // a race shows on some runs only: 30 rounds make one test run catch it
    for _ in 0..30 {
        let dir = std::env::temp_dir().join(format!("fael-start-{}", fael_core::ulid()));
        let path = dir.join("fael.db");
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
