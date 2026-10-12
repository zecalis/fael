//! `plan migrate`: an active db plan, a done md plan, an md plan not cut over, a spec by
//! Used by, a spec by name, an orphan; a second run writes nothing; a file db is copied first.

use super::super::section::list;
use super::super::{Import, Store, md};
use super::Spec;
use std::collections::HashMap;

const A: &str = "---\nkind: unit\n---\n\n# PLAN-a\n\n## TL;DR\nWhy in one line.\n\
                 - [ ] a1 — one\n\n## 1. Goal\nShip a.\n\n## 2. Scope\n**Do:** x\n";
const D: &str = "# PLAN-d\n\n## TL;DR\n- [x] d1 — done\n\n## 1. Goal\nOld goal.\n";
const W: &str = "# PLAN-w\n\n## TL;DR\n- [ ] w1 — still in the md\n\n## 1. Goal\nW.\n";

fn files() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        (".fapony/plan/PLAN-a.md", A),
        (".fapony/done/PLAN-d.md", D),
        (".fapony/plan/PLAN-w.md", W),
    ])
}

fn store(s: &mut Store) {
    let one = |dir: &str, name: &str, body: &str| Import {
        app: String::new(),
        dir: dir.into(),
        source: format!(".fapony/{dir}/{name}"),
        md: md::parse(name, body).unwrap(),
    };
    let all = [
        one("plan", "PLAN-a.md", A),
        one("done", "PLAN-d.md", D),
        one("plan", "PLAN-w.md", W),
    ];
    s.import_all(&all, "t0").unwrap();
    let a = s
        .plans()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "a")
        .unwrap();
    s.cut_over(a.id).unwrap();
    s.new_plan("", "fresh", "Fresh", "dev", "t0").unwrap();
}

fn specs() -> Vec<Spec> {
    let spec = |f: &str, text: &str| Spec {
        app: String::new(),
        path: format!(".fapony/spec/{f}"),
        text: text.into(),
    };
    vec![
        spec(
            "SPEC-access.md",
            "# SPEC-access\n> **Used by:** [PLAN-gone.md](x) · [PLAN-d.md](../done/PLAN-d.md) · \
             [PLAN-a.md](../plan/PLAN-a.md)\n\n## §1 Roles\nr\n\n## §2 Flags\nf\n",
        ),
        spec("SPEC-d.md", "# SPEC-d\n\n## §1 Rule\nd rule\n"),
        spec("SPEC-w.md", "## §1 Held\n"),
        spec("SPEC-orphan.md", "> **Used by:** PLAN-nope\n## §1 Lost\n"),
    ]
}

fn run(s: &mut Store, dry: bool) -> String {
    let f = files();
    s.migrate_md(&|p| f.get(p).map(|t| t.to_string()), &specs(), dry)
        .unwrap()
}

fn keys(s: &Store, name: &str) -> Vec<String> {
    let id = s
        .plans()
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .id;
    list(&s.conn, id)
        .unwrap()
        .into_iter()
        .map(|r| r.key)
        .collect()
}

fn truth(s: &Store, name: &str) -> String {
    s.plans()
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .truth
}

#[test]
fn migrate_moves_md_and_specs_into_sections_once() {
    let mut s = Store::open_in_memory().unwrap();
    store(&mut s);
    let dry = run(&mut s, true);
    assert!(dry.ends_with("dry run, nothing written"), "{dry}");
    assert!(
        keys(&s, "a").is_empty() && truth(&s, "d") == "md",
        "dry writes nothing"
    );

    let out = run(&mut s, false);
    let summary =
        "migrate: 4 plans, 9 sections, 2 specs attached, 1 unattached, 1 md plans → truth db";
    assert!(out.contains(summary), "{out}");
    assert_eq!(
        dry.lines().count(),
        out.lines().count(),
        "dry counts = the real run's"
    );
    // TL;DR prose kept, checkboxes dropped; the first open plan Used by names wins
    assert_eq!(
        keys(&s, "a"),
        ["tldr", "1", "2", "access:intro", "access:§1", "access:§2"]
    );
    let a = s
        .plans()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "a")
        .unwrap();
    let tldr = &list(&s.conn, a.id).unwrap()[0];
    assert_eq!(
        (tldr.title.as_str(), tldr.body.as_str()),
        ("TL;DR", "Why in one line.")
    );
    // the done md plan: its sections, the spec by name, truth flipped
    assert_eq!(keys(&s, "d"), ["1", "d:intro", "d:§1"]);
    assert_eq!(truth(&s, "d"), "db");
    // an md plan not cut over is left whole, its spec with it
    assert!(keys(&s, "w").is_empty() && truth(&s, "w") == "md");
    assert!(out.contains("fael plan cutover w` first"), "{out}");
    assert!(
        out.contains(".fapony/spec/SPEC-w.md skipped with the plan"),
        "{out}"
    );
    assert!(
        out.contains("unattached: .fapony/spec/SPEC-orphan.md — names no plan of this repo (used by: nope), and no plan 'orphan'"),
        "{out}"
    );
    assert!(
        out.contains("safe to delete: .fapony/plan/PLAN-a.md .fapony/spec/SPEC-access.md"),
        "{out}"
    );
    assert!(out.contains("fresh: md skipped — has 4 sections"), "{out}");

    let again = run(&mut s, false);
    assert!(
        again.contains("migrate: 4 plans, 0 sections, 0 specs attached"),
        "{again}"
    );
    assert!(again.ends_with("nothing to write"), "{again}");
    assert!(!again.contains("safe to delete"), "{again}");
    assert_eq!(keys(&s, "a").len(), 6);
}

#[test]
fn a_file_db_is_copied_before_the_commit() {
    let dir = std::env::temp_dir().join(format!("fael-migrate-{}", crate::ulid()));
    let path = dir.join("plans.db");
    let mut s = Store::open(&path).unwrap();
    store(&mut s);
    let out = run(&mut s, false);
    let bak = dir.join("plans.db.pre-migrate.bak");
    // sqlite names the db by its real path (/private/var on macOS)
    assert!(
        out.ends_with("/plans.db.pre-migrate.bak") && bak.exists(),
        "{out}"
    );
    let old = Store::open(&bak).unwrap();
    assert!(
        keys(&old, "a").is_empty(),
        "the copy is the db before migrate"
    );
    assert_eq!(truth(&old, "d"), "md");
    let _ = std::fs::remove_dir_all(dir);
}
