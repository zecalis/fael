//! Plan resolution (PLAN-fael-plan-focus chunk 1): L1 `open_plans` reads the
//! open `plan:<name>:chunk-<n>` rows into facts, L3 `resolve_plan` picks one
//! answer in the order declared > branch > only, and everything else is
//! `Ambiguous` — never a newest tiebreak.

use super::{log, row};
use fael_core::*;
use std::collections::BTreeSet;

fn plan_row(id: &str, key: &str, branch: &str) -> Row {
    let mut r = row(id, "note", &["src/a.rs"], Some(key));
    r.extra.insert("branch".into(), branch.into());
    r
}

fn fact(name: &str, chunk: u32, branch: &str) -> PlanFact {
    let mut chunks = BTreeSet::new();
    chunks.insert(chunk);
    let mut branches = BTreeSet::new();
    branches.insert(branch.to_string());
    PlanFact {
        name: name.to_string(),
        chunks,
        branches,
    }
}

fn facts(list: &[(&str, u32, &str)]) -> Vec<PlanFact> {
    list.iter().map(|(n, c, b)| fact(n, *c, b)).collect()
}

fn active(name: &str, chunk: Option<u32>, source: PlanSource) -> PlanResolution {
    PlanResolution::Active {
        name: name.to_string(),
        chunk,
        source,
    }
}

fn candidate(name: &str, chunk: u32) -> PlanCandidate {
    PlanCandidate {
        name: name.to_string(),
        chunk,
    }
}

#[test]
fn open_plans_groups_open_chunks_and_branches() {
    let a1 = plan_row("A0000000000000000000000040", "plan:foo:chunk-1", "feat/one");
    let a2 = plan_row("A0000000000000000000000041", "plan:foo:chunk-2", "feat/two");
    let b1 = plan_row("A0000000000000000000000042", "plan:bar:chunk-1", "feat/one");
    // one plan, two chunks, two branches; a second plan on one of them;
    // sorted by name, so `bar` comes before `foo`
    let facts = open_plans(&[&a1, &a2, &b1]);
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].name, "bar");
    assert_eq!(facts[1].name, "foo");
    assert_eq!(facts[1].chunks.iter().copied().collect::<Vec<_>>(), [1, 2]);
    assert_eq!(
        facts[1]
            .branches
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["feat/one", "feat/two"]
    );
}

#[test]
fn open_plans_ignores_keys_that_are_not_a_plan_chunk() {
    for key in ["plan:foo", "plan:foo:chunk-x", "plan::chunk-1"] {
        let r = row(
            "A0000000000000000000000033",
            "note",
            &["src/a.rs"],
            Some(key),
        );
        assert!(open_plans(&[&r]).is_empty(), "{key}");
    }
}

#[test]
fn closed_plan_rows_do_not_count() {
    // `find` hides the closed row (the caller's job), so only foo stays open
    let mut l = log();
    l.rows.push(plan_row(
        "A0000000000000000000000050",
        "plan:foo:chunk-1",
        "feat/x",
    ));
    l.rows.push(plan_row(
        "A0000000000000000000000051",
        "plan:bar:chunk-1",
        "feat/x",
    ));
    l.closes
        .push(Row::close("t-0000", "A0000000000000000000000051", "done"));
    let open = find(&l, &Filter::default());
    let facts = open_plans(&open);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].name, "foo");
}

#[test]
fn resolve_matches_the_matrix() {
    // B = the branch's own open plans · G = every open plan · I = intent.
    // 1. B=A, G=A → Active / branch
    assert_eq!(
        resolve_plan(&facts(&[("a", 1, "b")]), Some("b"), None),
        active("a", Some(1), PlanSource::Branch)
    );
    // 2. B=A, G=A,B → Active / branch (only A is on this branch)
    assert_eq!(
        resolve_plan(&facts(&[("a", 1, "b"), ("b", 2, "other")]), Some("b"), None),
        active("a", Some(1), PlanSource::Branch)
    );
    // 3. B=A,B, G=A,B → ambiguous, no newest tiebreak
    assert_eq!(
        resolve_plan(&facts(&[("a", 1, "b"), ("b", 2, "b")]), Some("b"), None),
        PlanResolution::Ambiguous {
            candidates: vec![candidate("a", 1), candidate("b", 2)]
        }
    );
    // 4. B=-, G=A → Active / only
    assert_eq!(
        resolve_plan(&facts(&[("a", 1, "other")]), Some("b"), None),
        active("a", Some(1), PlanSource::Only)
    );
    // 5. B=-, G=A,B → ambiguous
    assert_eq!(
        resolve_plan(&facts(&[("a", 1, "x"), ("b", 2, "y")]), Some("b"), None),
        PlanResolution::Ambiguous {
            candidates: vec![candidate("a", 1), candidate("b", 2)]
        }
    );
    // 6. B=-, G=- → none
    assert_eq!(
        resolve_plan(&facts(&[]), Some("b"), None),
        PlanResolution::None
    );
    // 7. B=A,B, G=A,B, I=A → declared wins
    assert_eq!(
        resolve_plan(
            &facts(&[("a", 1, "b"), ("b", 2, "b")]),
            Some("b"),
            Some("a")
        ),
        active("a", Some(1), PlanSource::Declared)
    );
    // 8. B=-, G=A,B, I=A → declared
    assert_eq!(
        resolve_plan(
            &facts(&[("a", 1, "x"), ("b", 2, "y")]),
            Some("b"),
            Some("a")
        ),
        active("a", Some(1), PlanSource::Declared)
    );
    // 9. B=-, G=-, I=A → declared, no open chunk
    assert_eq!(
        resolve_plan(&facts(&[]), Some("b"), Some("a")),
        active("a", None, PlanSource::Declared)
    );
    // 10. B=A, G=A,B, I=B → explicit beats inferred
    assert_eq!(
        resolve_plan(
            &facts(&[("a", 1, "b"), ("b", 2, "other")]),
            Some("b"),
            Some("b")
        ),
        active("b", Some(2), PlanSource::Declared)
    );
    // no branch at all: none, even with facts in play
    assert_eq!(
        resolve_plan(&facts(&[("a", 1, "b")]), None, None),
        PlanResolution::None
    );
    // no branch at all: none, even with a declared intent (detached HEAD
    // has no branch to bind it to)
    assert_eq!(
        resolve_plan(&facts(&[("a", 1, "b")]), None, Some("a")),
        PlanResolution::None
    );
}

#[test]
fn resolve_takes_the_highest_open_chunk_not_the_newest_row() {
    // a row filed later for chunk-2 must not pull the pointer back from chunk-4
    let mut chunks = BTreeSet::new();
    for c in [1u32, 2, 4] {
        chunks.insert(c);
    }
    let mut branches = BTreeSet::new();
    branches.insert("feat/x".to_string());
    let f = vec![PlanFact {
        name: "foo".into(),
        chunks,
        branches,
    }];
    assert_eq!(
        resolve_plan(&f, Some("feat/x"), None),
        active("foo", Some(4), PlanSource::Branch)
    );
}

#[test]
fn ambiguous_candidates_are_sorted_by_name() {
    let f = facts(&[("zeta", 1, "x"), ("alpha", 2, "y"), ("mid", 3, "z")]);
    match resolve_plan(&f, Some("none"), None) {
        PlanResolution::Ambiguous { candidates } => assert_eq!(
            candidates
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "mid", "zeta"]
        ),
        other => panic!("{other:?}"),
    }
}
