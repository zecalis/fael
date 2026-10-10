//! Re-import keeps uids (SPEC §7), never touches a `db` plan's chunks, and an export
//! reads back through the same grammar.

use super::super::{Fields, Ready, export, md};
use super::*;

fn plan(name: &str, lines: &str) -> Import {
    let file = format!("PLAN-{name}.md");
    Import {
        app: String::new(),
        dir: "plan".into(),
        source: format!(".fapony/plan/{file}"),
        md: md::parse(
            &file,
            &format!("# PLAN-{name}\n\n## TL;DR\n{lines}\n## 1. Goal\n"),
        )
        .unwrap(),
    }
}

fn uids(s: &Store, name: &str) -> Vec<String> {
    s.conn
        .prepare(
            "SELECT c.label || '=' || c.uid FROM chunk c JOIN plan p ON p.id = c.plan
             WHERE p.name = ?1 ORDER BY c.seq",
        )
        .unwrap()
        .query_map([name], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn uid_of(all: &[String], label: &str) -> String {
    all.iter()
        .find_map(|x| x.strip_prefix(&format!("{label}=")))
        .unwrap()
        .to_string()
}

#[test]
fn matching_by_label_then_title_never_guesses() {
    let k = |l: &str, t: &str| ((!l.is_empty()).then(|| l.to_string()), t.to_string());
    let old = [
        k("1", "a"),
        k("2", "b"),
        k("", "free"),
        k("3", "x"),
        k("3", "y"),
    ];
    let new = [
        k("1", "a, edited"), // label
        k("9", "b"),         // title
        k("", "free"),       // title, no label
        k("3", "x"),         // label 3 twice on the old side, title x unique: title
        k("4", "z"),         // new
        k("3", "w"),         // label 3 twice on both sides, title unknown: ambiguous
    ];
    let (hit, amb) = match_old(&old, &new);
    assert_eq!(hit, [Some(0), Some(1), Some(2), Some(3), None, None]);
    assert_eq!(amb, [5]);
    // one old chunk is never taken twice
    let (hit, amb) = match_old(&[k("1", "a")], &[k("1", "b"), k("2", "a")]);
    assert_eq!((hit, amb), (vec![Some(0), None], vec![1]));
}

#[test]
fn reimport_keeps_uids_and_edges_into_it() {
    let mut s = Store::open_in_memory().unwrap();
    let a = "- [x] 1 — first\n- [ ] 2 (wip feat/x) — second";
    let b = "- [ ] k1 (after a:2) — waits on a's 2";
    s.import_all(&[plan("a", a), plan("b", b)], "t0").unwrap();
    let before = uids(&s, "a");
    let runs = |s: &Store| -> String {
        s.conn
            .query_row(
                "SELECT COALESCE(group_concat(start || branch), '') FROM run",
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    let run = runs(&s);
    let rep = s.import_all(&[plan("a", a), plan("b", b)], "t1").unwrap();
    assert_eq!(uids(&s, "a"), before, "unchanged files keep every uid");
    assert_eq!(runs(&s), run, "and the (wip) run");
    assert!(rep.ambiguous.is_empty());
    // 2 is ticked and 3 inserted before it: 2 keeps its uid, b's edge still points at it
    let a2 = "- [x] 1 — first\n- [ ] 3 — new\n- [x] 2 — second — abc123";
    let rep = s.import_all(&[plan("a", a2), plan("b", b)], "t2").unwrap();
    let after = uids(&s, "a");
    assert_eq!(uid_of(&after, "2"), uid_of(&before, "2"));
    assert!(rep.unresolved.is_empty(), "{:?}", rep.unresolved);
    let n: i64 = s
        .conn
        .query_row(
            "SELECT COUNT(*) FROM event e JOIN chunk c ON c.uid = e.chunk_uid WHERE c.label = '2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2, "running, then done — no event for an unchanged state");
    assert_eq!(runs(&s).len(), 0, "a ticked chunk's run goes");
}

#[test]
fn a_db_plan_keeps_its_chunks() {
    let mut s = Store::open_in_memory().unwrap();
    s.import_all(
        &[plan("a", "- [ ] 1 — one"), plan("gone", "- [ ] 1 — x")],
        "t0",
    )
    .unwrap();
    s.conn
        .execute_batch(
            "UPDATE plan SET truth = 'db' WHERE name = 'a';
             UPDATE chunk SET state = 'review' WHERE label = '1';",
        )
        .unwrap();
    let before = uids(&s, "a");
    let mut edited = plan("a", "- [x] 1 — one\n- [ ] 2 — two");
    edited.md.title = "PLAN-a — renamed".into();
    let rep = s.import_all(&[edited], "t1").unwrap();
    assert_eq!((rep.db_plans, rep.chunks), (1, 0));
    assert_eq!(uids(&s, "a"), before);
    let (title, state): (String, String) = s
        .conn
        .query_row(
            "SELECT p.title, c.state FROM plan p JOIN chunk c ON c.plan = p.id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (title.as_str(), state.as_str()),
        ("PLAN-a — renamed", "review")
    );
    // an md plan whose file is gone goes; the db plan stays
    let names: Vec<String> = s.plans().unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["a"]);
}

#[test]
fn export_reads_back() {
    let mut s = Store::open_in_memory().unwrap();
    let lines = "- [x] 1 — done\n- [ ] 2 (wip feat/x) — running\n- [~] 3 — dropped\n- [?] 4 — odd\n- [ ] 5 (after 1) — open";
    s.import_all(&[plan("a", lines)], "t0").unwrap();
    let id = s.plans().unwrap()[0].id;
    let out = export(&s, id).unwrap();
    assert!(out.contains("· running · after 1 · run "), "{out}");
    assert!(out.contains("· open · after 1"), "{out}");
    let back = md::parse("PLAN-a.md", &out).unwrap();
    let orig = md::parse("PLAN-a.md", &format!("# x\n\n## TL;DR\n{lines}\n")).unwrap();
    let ticks = |p: &md::PlanMd| {
        p.items
            .iter()
            .map(|i| (i.tick, i.label.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(ticks(&back), ticks(&orig));
    assert_eq!(back.title, "PLAN-a");
}

#[test]
fn an_md_after_names_a_db_plan_chunk() {
    let mut s = Store::open_in_memory().unwrap();
    let files = [
        plan("a", "- [ ] 1 (after b:2) — waits on b"),
        plan("b", "- [x] 1 — one\n- [ ] 2 — two"),
    ];
    s.import_all(&files, "t0").unwrap();
    s.conn
        .execute("UPDATE plan SET truth = 'db' WHERE name = 'b'", [])
        .unwrap();
    // b's chunks now come from db_chunks(): the edge still lands on b:2
    let rep = s.import_all(&files, "t1").unwrap();
    assert_eq!((rep.db_plans, rep.unresolved.len()), (1, 0));
    let a1 = uids(&s, "a")[0].split_once('=').unwrap().1.to_string();
    let b2 = uids(&s, "b")[1].split_once('=').unwrap().1.to_string();
    match s.ready(&a1, "2026-10-11").unwrap() {
        Ready::Blocked(u) => assert_eq!(u[0].uid.as_deref(), Some(b2.as_str())),
        r => panic!("{r:?}"),
    }
    s.conn
        .execute("UPDATE chunk SET state = 'done' WHERE uid = ?1", [&b2])
        .unwrap();
    assert_eq!(s.ready(&a1, "2026-10-11").unwrap(), Ready::Yes);
}

#[test]
fn a_db_chunk_after_an_md_chunk_survives_reimport() {
    let mut s = Store::open_in_memory().unwrap();
    let a = |tick: &str| plan("a", &format!("- [{tick}] 1 — md work"));
    s.import_all(&[a(" "), plan("b", "- [x] 0 — old")], "t0")
        .unwrap();
    s.conn
        .execute("UPDATE plan SET truth = 'db' WHERE name = 'b'", [])
        .unwrap();
    let a1 = uids(&s, "a")[0].split_once('=').unwrap().1.to_string();
    let f = Fields {
        title: Some("db work".into()),
        brief: Some("after the md chunk".into()),
        ..Fields::default()
    };
    let c = s.add("b", &f, std::slice::from_ref(&a1), "t0").unwrap();
    s.import_all(&[a(" "), plan("b", "")], "t1").unwrap();
    assert!(matches!(
        s.ready(&c, "2026-10-11").unwrap(),
        Ready::Blocked(_)
    ));
    // the md tick reaches the db chunk through the same edge
    s.import_all(&[a("x"), plan("b", "")], "t2").unwrap();
    assert_eq!(s.ready(&c, "2026-10-11").unwrap(), Ready::Yes);
}
