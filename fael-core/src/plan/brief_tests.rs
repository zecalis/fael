//! The brief: the md fallback by heading, refs, pairs, notes and hand-offs; reads and the
//! section index are in `section_tests.rs` (they need a store).

use super::super::start::{Brief, Started};
use super::*;

const DOC: &str = "# PLAN-x\n\n## TL;DR\n- [ ] 1 — one\n\n## 1. Goal\nship it\n\n\
                   ## 2. Scope\nonly cli\n\n## 3. Done criteria\ntests pass\n\n\
                   ## 4. Constraints\n400 lines\n\n## 5. Risks\nnone said\n";

fn chunk(uid: &str) -> Brief {
    Brief {
        uid: uid.into(),
        label: Some("1".into()),
        title: format!("title {uid}"),
        brief: format!("brief {uid}"),
        size: None,
        model_hint: None,
        client_hint: None,
        scope: None,
        plan: "x".into(),
        plan_title: "PLAN-x".into(),
        source: ".fapony/plan/PLAN-x.md".into(),
        sections: vec![],
        reads: vec![],
        repo: None,
        root: None,
        refs: vec!["SPEC-x.md#block".into(), "gone.rs".into()],
        said: vec![],
        notes: vec![],
        after: vec![],
        resume: None,
        issues: vec![],
    }
}

#[test]
fn the_brief_reads_the_plan_md_live_and_marks_missing_refs() {
    let s = sections(DOC);
    for keep in [
        "## 1. Goal\nship it",
        "## 2. Scope",
        "## 3. Done criteria",
        "## 4. Constraints\n400 lines",
    ] {
        assert!(s.contains(keep), "{keep} in {s}");
    }
    assert!(!s.contains("TL;DR") && !s.contains("Risks"), "{s}");
    let st = Started {
        run: "R".into(),
        chunks: vec![chunk("A"), chunk("B")],
    };
    let md = |src: &str| (src == ".fapony/plan/PLAN-x.md").then(|| DOC.to_string());
    let t = st.text(&md, &|p| p == "SPEC-x.md");
    assert!(t.starts_with("fael run R · 2 paired chunks\n"), "{t}");
    assert!(t.contains("\n# 1 — title A (A)\n"), "{t}");
    let mut imported = chunk("C");
    imported.title = "1 — from the md".into();
    let one = Started {
        run: "R".into(),
        chunks: vec![imported],
    };
    assert!(
        one.text(&|_| None, &|_| true)
            .contains("\n# 1 — from the md (C)\n")
    );
    assert!(t.contains("brief A") && t.contains("brief B"), "{t}");
    assert_eq!(
        t.matches("## 1. Goal").count(),
        1,
        "plan context once per plan: {t}"
    );
    assert!(
        t.contains("refs: SPEC-x.md#block · gone.rs (missing)"),
        "{t}"
    );
    assert!(t.contains("fael chunk done A"), "{t}");
    // what the agent leaves for the owner fits one board line
    assert!(
        t.contains("first sentence = the outcome") && t.contains("first line <= 100 chars"),
        "{t}"
    );
    let mut noted = chunk("D");
    noted.notes = vec!["use v2\nnot v1".into()];
    noted.after = vec![("b3b".into(), "board ships".into())];
    let t2 = Started {
        run: "R".into(),
        chunks: vec![noted],
    }
    .text(&|_| None, &|_| true);
    assert!(
        t2.contains("\nnotes left for this chunk:\n> use v2\n> not v1\n")
            && t2.contains("\nb3b, before this, handed off:\n> board ships\n"),
        "{t2}"
    );
    // a plan with sections reads them, not the md file
    let mut own = chunk("E");
    own.sections = vec![Row {
        key: "1".into(),
        title: "1. Goal".into(),
        body: "from the db".into(),
        updated: "2026-10-12T00:00:00Z".into(),
    }];
    let t3 = Started {
        run: "R".into(),
        chunks: vec![own],
    }
    .text(&md, &|_| true);
    assert!(
        t3.contains(
            "<!-- plan x -->\n\n## 1. Goal\n> section 1 · updated 2026-10-12\n\nfrom the db\n"
        ) && !t3.contains("ship it"),
        "{t3}"
    );
    // no plan file (inbox, or a moved md): the brief still prints
    let t = st.text(&|_| None, &|_| true);
    assert!(
        !t.contains("Goal") && t.contains("refs: SPEC-x.md#block · gone.rs\n"),
        "{t}"
    );
}

#[test]
fn the_rules_ask_for_the_windows_ci_result_not_the_trigger() {
    let r = rules("U1");
    assert!(r.contains("gh run watch <id> --exit-status"), "{r}");
    assert!(
        r.contains("fail with the failing test, in the done text"),
        "{r}"
    );
}
