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
        by_file: vec![],
        by_file_more: 0,
        live: vec![],
        live_more: 0,
        resume: None,
        issues: vec![],
    }
}

#[test]
fn the_brief_prints_the_plan_sections_once() {
    let st = Started {
        run: "R".into(),
        chunks: vec![chunk("A"), chunk("B")],
        orphaned: vec![],
        resumed: false,
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
        orphaned: vec![],
        resumed: false,
    };
    assert!(one.text().contains("\n# 1 — titled with its label (C)\n"));
    assert!(t.contains("brief A") && t.contains("brief B"), "{t}");
    assert_eq!(
        t.matches("## 1. Goal").count(),
        1,
        "plan context once per plan: {t}"
    );
    assert!(t.contains("fael chunk done A"), "{t}");
    assert!(
        t.contains("TLDR: do \"brief A\" then `fael chunk done A \"<handoff>\" --pr N`"),
        "{t}"
    );
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
        orphaned: vec![],
        resumed: false,
    }
    .text();
    assert!(
        t2.contains("\nnotes left for this chunk:\n> use v2\n> not v1\n")
            && t2.contains(
                "\nb3b, before this, handed off:\n> board ships\nfiles it touched: a/x.rs, b/y.rs\n"
            )
            && t2.contains(
                "\nb3c, before this, handed off:\n> no files\nfiles it touched: none claimed\n"
            )
            && t2.matches("files it touched").count() == 2,
        "{t2}"
    );
    // a plan with no sections (inbox): the brief still prints
    let mut bare = chunk("E");
    bare.sections = vec![];
    let t = Started {
        run: "R".into(),
        chunks: vec![bare],
        orphaned: vec![],
        resumed: false,
    }
    .text();
    assert!(!t.contains("Goal") && t.contains("brief E"), "{t}");
}

#[test]
fn the_rules_keep_only_what_every_chunk_needs() {
    // issue 01M5Y6H4: the situational rules are said by their verb or in `--full`
    let r = rules("U1", None);
    let bullets = r.lines().filter(|l| l.starts_with("- ")).count();
    assert!(bullets <= 8, "{bullets} bullets: {r}");
    for gone in [
        "lane: one scope",
        "Recommend:",
        "--on data names",
        "two lines of Done",
        "makes untrue",
        "Windows CI",
        "gh workflow run",
    ] {
        assert!(!r.contains(gone), "{gone} in {r}");
    }
    assert!(r.ends_with("`fael chunk start U1 --full`\n"), "{r}");
    // the Windows CI rule belongs to the fael repo's own AGENTS.md, not to every repo
    assert!(!rules_full("U1", None).contains("gh workflow run"));
}

#[test]
fn full_rules_are_the_core_plus_the_situational_ones() {
    let f = rules_full("U1", None);
    let core = rules("U1", None);
    // everything in core but its closing pointer line opens the full text
    let (head, _) = core.trim_end().rsplit_once('\n').unwrap();
    assert!(f.starts_with(head), "{f}");
    for kept in [
        "lane: one scope per worktree",
        "Recommend: <option> because <evidence>",
        "a wait --on data names what is measured",
        "serve two lines of Done",
        "makes untrue",
    ] {
        assert!(f.contains(kept), "{kept} in {f}");
    }
    assert!(!core.contains("serve two lines of Done"));
}

#[test]
fn start_full_prints_the_full_rules_and_the_lean_start_the_core() {
    let st = Started {
        run: "R".into(),
        chunks: vec![chunk("A")],
        orphaned: vec![],
        resumed: false,
    };
    assert!(st.render(true, true).contains("lane: one scope"));
    assert!(!st.render(false, true).contains("lane: one scope"));
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
        orphaned: vec![],
        resumed: false,
    }
    .text();
    assert!(t.contains("## 1. Goal"), "{t}");
    // a blank Done names its fill command instead of printing nothing;
    // Scope and Risks stay suppressed
    assert!(
        t.contains(
            "section 3 (3. Done criteria) is still blank — fill it: `fael plan export x --json`"
        ),
        "{t}"
    );
    for gone in ["2. Scope", "5. Risks"] {
        assert!(!t.contains(gone), "{gone} in {t}");
    }
    c = chunk("B");
    c.sections = vec![sec("1", "1. Goal", "")];
    let t = Started {
        run: "R".into(),
        chunks: vec![c],
        orphaned: vec![],
        resumed: false,
    }
    .text();
    // a blank Goal still prints the plan header with its fill pointer
    assert!(t.contains("<!-- plan x -->"), "{t}");
    assert!(
        t.contains("section 1 (1. Goal) is still blank — fill it: `fael plan export x --json`"),
        "{t}"
    );
}

#[test]
fn the_rules_close_with_one_done_form_from_the_worktree() {
    let r = rules("U1", None);
    assert!(
        r.contains("`fael chunk done \"<result>\" --left none|<ids>`"),
        "{r}"
    );
    assert!(!r.contains("Fael-Chunk"), "{r}");
}

#[test]
fn the_rules_name_review_flag_stdin_and_amend() {
    let r = rules("U1", None);
    assert!(r.contains("`--review clean|fixed|failed`"), "{r}");
    assert!(!r.contains("verdict in the done text"), "{r}");
    assert!(r.contains("fael chunk done - --left none <<'EOF'"), "{r}");
    assert!(r.contains("amends the hand-off"), "{r}");
}

/// The add lines carry the full form, the gate's title and one-topic rules in
/// it: an elided `fael add issue …` sent agents to `fael add --help` first.
#[test]
fn the_rules_spell_out_the_full_add_form() {
    let r = rules("U1", None);
    assert!(
        !r.contains("fael add issue …") && !r.contains("fael add note …"),
        "{r}"
    );
    assert!(
        r.contains(
            "`fael add issue \"<one topic, no ; list>\" --title \"<≤15 words>\" --files <f>`"
        ),
        "{r}"
    );
    assert!(
        r.contains("`fael add note \"<state>\" --title \"<≤15 words>\" --files <f>`"),
        "{r}"
    );
}

#[test]
fn the_brief_opens_each_chunk_with_a_tldr_naming_done() {
    let t = Started {
        run: "R".into(),
        chunks: vec![chunk("A")],
        orphaned: vec![],
        resumed: false,
    }
    .text();
    assert!(
        t.contains("TLDR: do \"brief A\" then `fael chunk done A \"<handoff>\" --pr N`"),
        "{t}"
    );
    // a chunk without its own brief falls back to the title
    let mut bare = chunk("B");
    bare.brief = bare.title.clone();
    let t = Started {
        run: "R".into(),
        chunks: vec![bare],
        orphaned: vec![],
        resumed: false,
    }
    .text();
    assert!(
        t.contains("TLDR: do \"title B\" then `fael chunk done B \"<handoff>\" --pr N`"),
        "{t}"
    );
}

#[test]
fn the_rules_state_the_lane_contract_info_only() {
    let r = rules_full("U1", None);
    assert!(
        r.contains("land in plan order, first-listed chunk first"),
        "{r}"
    );
    assert!(r.contains("the owner merges every PR"), "{r}");
    assert!(r.contains("fael never enforces an order"), "{r}");
    assert!(r.contains("fael run end"), "{r}");
    assert!(r.contains("outside declared scope"), "{r}");
    assert!(r.contains("overlaps open chunk"), "{r}");
    assert!(r.contains("never a refusal"), "{r}");
}

#[test]
fn only_a_mixed_chunk_gets_the_delegate_line() {
    let mixed = rules("U1", Some("mixed"));
    assert!(mixed.contains("`model: sonnet`"), "{mixed}");
    assert!(mixed.contains("before each step commit"), "{mixed}");
    for m in [None, Some("opus"), Some("sonnet")] {
        assert!(!rules("U1", m).contains("small-model subagents"), "{m:?}");
    }
    let mut c = chunk("A");
    c.model = Some("mixed".into());
    let t = Started {
        run: "R".into(),
        chunks: vec![c],
        orphaned: vec![],
        resumed: false,
    }
    .text();
    assert!(t.contains("small-model subagents"), "{t}");
}

#[test]
fn the_default_brief_is_handoff_and_tldr_with_rules_once() {
    let mut c = chunk("A");
    c.after = vec![("P prev".into(), "it works now".into(), String::new())];
    let st = Started {
        run: "R".into(),
        chunks: vec![c],
        orphaned: vec![],
        resumed: false,
    };
    let lean = st.render(false, false);
    assert!(
        !lean.contains("ship it") && !lean.contains("## chunk rules\n"),
        "{lean}"
    );
    assert!(lean.contains("`fael chunk start A --full`"), "{lean}");
    let (h, t) = (lean.find("it works now"), lean.find("TLDR:"));
    assert!(h < t, "hand-off before TLDR: {lean}");
    assert!(lean.contains("--left none|<ids>"), "{lean}");
    assert!(st.render(false, true).contains("## chunk rules\n"));
    assert!(st.render(true, true).contains("ship it"));
    assert_eq!(st.text(), st.render(true, true));
}

/// c5 precedence: resumed short renders run → TLDR → resume → `--full`
/// pointer only. Sections, answers, notes, hand-offs and held rows stay in the
/// first brief — suppression by the session invariant, never by rank.
#[test]
fn start_resume_short_orders_run_tldr_resume_pointer() {
    let mut c = chunk("A");
    c.resume = Some(super::super::start::Prev {
        start: "R".into(),
        client: Some("claude".into()),
        session: Some("S1".into()),
        worktree: Some("/w".into()),
        branch: Some("main".into()),
        started: "2026-10-30T00:00:00Z".into(),
        last_seen: None,
    });
    let st = Started {
        run: "R".into(),
        chunks: vec![c],
        orphaned: vec![],
        resumed: true,
    };
    let short = st.render_short();
    let (run, tldr, resume, ptr) = (
        short.find("fael run R"),
        short.find("TLDR:"),
        short.find("resume — run R"),
        short.find("--full"),
    );
    assert!(
        run < tldr && tldr < resume && resume < ptr,
        "run → TLDR → resume → pointer: {short}"
    );
    for gone in [
        "## 1. Goal",
        "## chunk rules",
        "owner said",
        "files it touched",
    ] {
        assert!(
            !short.contains(gone),
            "{gone} stays in the first brief: {short}"
        );
    }
}
