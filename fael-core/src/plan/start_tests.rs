//! `chunk start`: one winner of a race, a pair all-or-none, per-member commands end only
//! that member's run (SPEC §4 test matrix), and the md-plan refusal.

use super::super::chunk::tests::{NOW, add, get, here, live, start, state};
use super::super::{Fields, Here, Import, Owner, Start, Store, md};
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
                "wait" => s.wait(m, "owner", "q", None, &h),
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
    assert!(st.text(&|_| None, &|_| true).contains("2 paired chunks"));
}

#[test]
fn two_starts_of_one_chunk_one_wins() {
    // a race shows on some runs only: 30 rounds make one test run catch it
    for _ in 0..30 {
        let dir = std::env::temp_dir().join(format!("fael-start-{}", crate::ulid()));
        let path = dir.join("plans.db");
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
fn an_md_plan_takes_no_chunk_command() {
    let mut s = Store::open_in_memory().unwrap();
    let doc = "# PLAN-x\n\n## TL;DR\n- [ ] 1 — one\n\n## 1. Goal\n";
    let imp = Import {
        app: String::new(),
        dir: "plan".into(),
        source: ".fapony/plan/PLAN-x.md".into(),
        md: md::parse("PLAN-x.md", doc).unwrap(),
    };
    s.import_all(&[imp], NOW).unwrap();
    let uid: String = s
        .conn
        .query_row("SELECT uid FROM chunk", [], |r| r.get(0))
        .unwrap();
    let e = start(&mut s, &uid, "/w").unwrap_err();
    assert!(e.contains("md plan x") && e.contains("cutover"), "{e}");
    let f = super::super::Fields {
        title: Some("t".into()),
        ..Default::default()
    };
    assert!(s.add("x", &f, &[], NOW).unwrap_err().contains("is md"));
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
fn client_hint_is_a_free_string_size_is_s_m_or_l() {
    let mut s = Store::open_in_memory().unwrap();
    let f = |size: &str, client: &str| Fields {
        title: Some("t".into()),
        size: Some(size.into()),
        client: Some(client.into()),
        ..Fields::default()
    };
    let c = s.add("inbox", &f("S", "opencode"), &[], NOW).unwrap();
    let e = s.add("inbox", &f("XL", "x"), &[], NOW).unwrap_err();
    assert!(e.contains("--size XL — S, M or L"), "{e}");
    s.approve(&c, Some(("deepseek", "opencode")), NOW).unwrap();
    s.edit(&c, &f("L", "claude"), NOW).unwrap();
    let q = "SELECT size || ' ' || client_hint || ' ' || COALESCE(approved_at, 'unapproved')
             FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &c).as_deref(), Some("L claude unapproved"));
}
