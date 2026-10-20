//! The brief: plan sections once per plan, pairs, notes and hand-offs; reads and the
//! section index are in `section_tests.rs` (they need a store).

use super::super::start::{Brief, Started};
use super::*;

fn sec(key: &str, title: &str, body: &str) -> Row {
    Row {
        key: key.into(),
        title: title.into(),
        body: body.into(),
        updated: "2026-10-12T00:00:00Z".into(),
    }
}

fn chunk(uid: &str) -> Brief {
    Brief {
        uid: uid.into(),
        label: Some("1".into()),
        title: format!("title {uid}"),
        brief: format!("brief {uid}"),
        size: None,
        model: None,
        scope: None,
        plan: "x".into(),
        plan_title: "x".into(),
        sections: vec![
            sec("1", "1. Goal", "ship it"),
            sec("2", "2. Scope", "only cli"),
            sec("3", "3. Done criteria", "tests pass"),
            sec("4", "4. Constraints", "400 lines"),
            sec("5", "5. Risks", "none said"),
        ],
        reads: vec![],
        said: vec![],
        notes: vec![],
        after: vec![],
        resume: None,
        issues: vec![],
    }
}

#[test]
fn the_brief_prints_the_plan_sections_once() {
    let st = Started {
        run: "R".into(),
        chunks: vec![chunk("A"), chunk("B")],
    };
    let t = st.text();
    assert!(t.starts_with("fael run R · 2 paired chunks\n"), "{t}");
    assert!(t.contains("\n# 1 — title A (A)\n"), "{t}");
    for keep in [
        "<!-- plan x -->\n\n## 1. Goal\n> section 1 · updated 2026-10-12\n\nship it\n",
        "## 2. Scope",
        "## 3. Done criteria",
        "## 4. Constraints",
        "- 5 · 5. Risks · 2026-10-12",
    ] {
        assert!(t.contains(keep), "{keep} in {t}");
    }
    assert!(
        !t.contains("none said"),
        "Risks is indexed, not printed: {t}"
    );
    let mut labelled = chunk("C");
    labelled.title = "1 — titled with its label".into();
    let one = Started {
        run: "R".into(),
        chunks: vec![labelled],
    };
    assert!(one.text().contains("\n# 1 — titled with its label (C)\n"));
    assert!(t.contains("brief A") && t.contains("brief B"), "{t}");
    assert_eq!(
        t.matches("## 1. Goal").count(),
        1,
        "plan context once per plan: {t}"
    );
    assert!(t.contains("fael chunk done A"), "{t}");
    // what the agent leaves for the owner fits one board line
    assert!(
        t.contains("first sentence = the outcome") && t.contains("first line <= 100 chars"),
        "{t}"
    );
    let mut noted = chunk("D");
    noted.notes = vec!["use v2\nnot v1".into()];
    noted.after = vec![
        ("b3b".into(), "board ships".into(), "a/x.rs\nb/y.rs".into()),
        ("b3c".into(), "no files".into(), String::new()),
    ];
    let t2 = Started {
        run: "R".into(),
        chunks: vec![noted],
    }
    .text();
    assert!(
        t2.contains("\nnotes left for this chunk:\n> use v2\n> not v1\n")
            && t2.contains(
                "\nb3b, before this, handed off:\n> board ships\nfiles it touched: a/x.rs, b/y.rs\n"
            )
            && t2.contains("\nb3c, before this, handed off:\n> no files\n")
            && t2.matches("files it touched").count() == 1,
        "{t2}"
    );
    // a plan with no sections (inbox): the brief still prints
    let mut bare = chunk("E");
    bare.sections = vec![];
    let t = Started {
        run: "R".into(),
        chunks: vec![bare],
    }
    .text();
    assert!(!t.contains("Goal") && t.contains("brief E"), "{t}");
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

#[test]
fn a_section_left_as_plan_new_wrote_it_is_not_printed() {
    let mut c = chunk("A");
    c.sections = vec![
        sec("1", "1. Goal", "ship it"),
        sec("2", "2. Scope", "**Do:**\n**Don't:**"),
        sec("3", "3. Done criteria", "  "),
        sec("5", "5. Risks", ""),
    ];
    let t = Started {
        run: "R".into(),
        chunks: vec![c],
    }
    .text();
    assert!(t.contains("## 1. Goal"), "{t}");
    for gone in ["2. Scope", "3. Done", "5. Risks"] {
        assert!(!t.contains(gone), "{gone} in {t}");
    }
    c = chunk("B");
    c.sections = vec![sec("1", "1. Goal", "")];
    let t = Started {
        run: "R".into(),
        chunks: vec![c],
    }
    .text();
    assert!(
        !t.contains("<!-- plan x -->"),
        "no plan header over nothing: {t}"
    );
}

#[test]
fn the_rules_tell_a_git_ship_agent_to_leave_the_handoff_as_a_finished_note() {
    let r = rules("U1");
    assert!(
        r.contains("`fael chunk note U1 \"finished: <handoff>\"`"),
        "{r}"
    );
    assert!(r.contains("`Fael-Chunk: U1`"), "{r}");
}
