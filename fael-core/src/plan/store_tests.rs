use super::super::{Next, next};
use super::*;
use std::collections::HashSet;

fn plan(app: &str, dir: &str, file: &str, body: &str) -> Import {
    Import {
        app: app.into(),
        dir: dir.into(),
        source: format!("{app}/.fapony/{dir}/{file}"),
        md: md::parse(file, body).unwrap(),
    }
}

fn tldr(lines: &str) -> String {
    format!("# PLAN-t\n\n## TL;DR\n{lines}\n## 1. Goal\n")
}

fn next_of(s: &Store, name: &str, branch: Option<&str>, live: Option<&HashSet<String>>) -> Next {
    let id = s
        .plans()
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .id;
    next(s, id, branch, live).unwrap()
}

fn label(n: Next) -> String {
    match n {
        Next::Chunk { label, .. } => label.unwrap_or_default(),
        other => format!("{other:?}"),
    }
}

#[test]
fn picks_like_fapony() {
    let mut s = Store::open_in_memory().unwrap();
    let body = tldr(
        "- [x] c1 — done
- [ ] c2 (wip feat/a) — claimed by another worktree
- [ ] c3 — waits on c2 (no marker)
- [ ] c4 (after c1) — ready alongside
- [ ] c5 (wait owner) — waits on a person",
    );
    let rep = s
        .import_all(&[plan("", "plan", "PLAN-t.md", &body)], "t0")
        .unwrap();
    assert_eq!((rep.plans, rep.chunks, rep.drafts), (1, 5, 0));
    // c2 is held by a live worktree: next is c4
    let live: HashSet<String> = ["feat/a".to_string()].into();
    assert_eq!(label(next_of(&s, "t", Some("main"), Some(&live))), "c4");
    // this session's own claim wins
    assert_eq!(label(next_of(&s, "t", Some("feat/a"), Some(&live))), "c2");
    // a claim no worktree holds is stale: c2 is picked like an open chunk
    assert_eq!(
        label(next_of(&s, "t", Some("main"), Some(&HashSet::new()))),
        "c2"
    );
}

#[test]
fn split_siblings_cross_plan_and_unresolved() {
    let mut s = Store::open_in_memory().unwrap();
    let a = tldr(
        "- [x] 2a — part
- [ ] 2b — part
- [ ] 3 (after 2) — waits on every part of 2
- [ ] 4 (after —) — free",
    );
    let b = tldr(
        "- [ ] x1 (after a:2b) — waits on a's 2b
- [ ] x2 (after gone:1, shipped:9) — unresolved + met by done/",
    );
    let shipped = tldr("- [ ] 1 — whatever");
    let rep = s
        .import_all(
            &[
                plan("apps/v", "plan", "PLAN-a.md", &a),
                plan("apps/v", "plan", "PLAN-b.md", &b),
                plan("apps/v", "done", "PLAN-shipped.md", &shipped),
            ],
            "t0",
        )
        .unwrap();
    assert_eq!(rep.unresolved, ["apps/v/b x2 → gone:1"]);
    assert_eq!(label(next_of(&s, "a", None, None)), "2b");
    assert_eq!(label(next_of(&s, "b", None, None)), "NoneReady");
    assert_eq!(label(next_of(&s, "shipped", None, None)), "Shipped");
}

#[test]
fn reimport_replaces_and_unknown_is_draft() {
    let mut s = Store::open_in_memory().unwrap();
    let one = tldr("- [?] q1 — odd\n- [ ] r1 — open");
    let rep = s
        .import_all(&[plan("", "plan", "PLAN-t.md", &one)], "t0")
        .unwrap();
    assert_eq!(rep.drafts, 1);
    // an unknown line is nobody's "chunk before": r is ready
    assert_eq!(label(next_of(&s, "t", None, None)), "r1");
    let two = tldr("- [x] r1 — closed");
    s.import_all(&[plan("", "plan", "PLAN-t.md", &two)], "t1")
        .unwrap();
    assert_eq!(s.plans().unwrap().len(), 1);
    assert_eq!(label(next_of(&s, "t", None, None)), "Closed");
    let blocked = format!("---\nstatus: blocked\n---\n{}", tldr("- [ ] r — open"));
    s.import_all(&[plan("", "plan", "PLAN-t.md", &blocked)], "t2")
        .unwrap();
    assert_eq!(label(next_of(&s, "t", None, None)), "Blocked");
}

#[test]
fn a_newer_db_is_refused_and_reopen_keeps_rows() {
    let dir = std::env::temp_dir().join(format!("fael-plans-{}", crate::ulid()));
    let path = dir.join("plans.db");
    let mut s = Store::open(&path).unwrap();
    s.import_all(
        &[plan("", "plan", "PLAN-t.md", &tldr("- [ ] r — open"))],
        "t0",
    )
    .unwrap();
    drop(s);
    assert_eq!(Store::open(&path).unwrap().plans().unwrap().len(), 1);
    let c = Connection::open(&path).unwrap();
    c.pragma_update(None, "user_version", 99).unwrap();
    drop(c);
    let e = Store::open(&path).err().unwrap();
    assert!(e.contains("v99") && e.contains("fael upgrade"), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}
