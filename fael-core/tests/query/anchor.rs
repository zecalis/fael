//! kickoff widening — `<PREFIX><name>.md` filters also match `<prefix>:<name>` anchors;
//! a docs-only row is told the anchor to add.

use super::{files, ids, log, row};
use fael_core::*;

#[test]
fn kickoff_matches_plan_anchor() {
    let r = std::env::temp_dir().join(format!("fael-kick-plan-{}", ulid()));
    std::fs::create_dir_all(r.join(".fapony/plan")).unwrap();
    std::fs::write(r.join(".fapony/plan/PLAN-foo.md"), "plan").unwrap();
    let l = Log {
        rows: vec![
            row("A0000000000000000000000010", "note", &["plan:foo"], None),
            row(
                "A0000000000000000000000011",
                "note",
                &[".fapony/plan/PLAN-bar.md"],
                None,
            ),
        ],
        closes: vec![],
        warnings: vec![],
    };
    // the PLAN path widens to its `plan:<name>` anchor; the other plan stays out
    let f = Filter {
        files: vec![".fapony/plan/PLAN-foo.md".into()],
        ..Filter::default()
    };
    assert_eq!(
        ids(&kickoff(&l, &f, &r, &Aliases::default(), &["PLAN-".into()])),
        ["10"]
    );
    // a PLAN- name ending mid multi-byte char widens to nothing, no panic
    assert!(
        kickoff(
            &l,
            &files(&["PLAN-แผน1"]),
            &r,
            &Aliases::default(),
            &["PLAN-".into()]
        )
        .is_empty()
    );
    // a non-plan query never matches the anchor
    assert!(
        kickoff(
            &l,
            &files(&["src/a.rs"]),
            &r,
            &Aliases::default(),
            &["PLAN-".into()]
        )
        .is_empty()
    );
    // a configured second prefix widens the same way; unconfigured it stays out
    std::fs::write(r.join("HANDOFF-req.md"), "handoff").unwrap();
    let l2 = Log {
        rows: vec![row(
            "A0000000000000000000000012",
            "note",
            &["handoff:req"],
            None,
        )],
        closes: vec![],
        warnings: vec![],
    };
    let h = Filter {
        files: vec!["HANDOFF-req.md".into()],
        ..Filter::default()
    };
    assert_eq!(
        ids(&kickoff(
            &l2,
            &h,
            &r,
            &Aliases::default(),
            &["PLAN-".into(), "HANDOFF-".into()]
        )),
        ["12"]
    );
    assert!(kickoff(&l2, &h, &r, &Aliases::default(), &["PLAN-".into()]).is_empty());
}

#[test]
fn docs_only_without_anchor_warns() {
    let l = log();
    let cfg = Config::default();
    let mk = |files: &[&str]| {
        let mut r = row("D0000000000000000000000017", "note", files, Some("k:v"));
        r.text = "about a doc".into();
        r
    };
    let w = warnings(&mk(&["spec/x.md", "notes/y.md"]), &l, &cfg);
    assert!(w[0].contains("only *.md docs"), "{w:?}");
    // the anchor comes ready to paste: a plan by the plan convention, any
    // other doc by its name
    assert!(
        w[0].ends_with("--files spec/x.md,notes/y.md,doc:x,doc:y"),
        "{w:?}"
    );
    let w = warnings(&mk(&["PRODUCT.md", "apps/v/PLAN-Vela.md"]), &l, &cfg);
    assert!(
        w[0].ends_with("--files PRODUCT.md,apps/v/PLAN-Vela.md,doc:product,plan:vela"),
        "{w:?}"
    );
    // a code file beside the docs is a lasting foothold — silent
    assert!(warnings(&mk(&["src/a.rs", "spec/x.md"]), &l, &cfg).is_empty());
    // an anchor never goes, so it anchors the row by itself too
    assert!(warnings(&mk(&["spec/x.md", "doc:pricing"]), &l, &cfg).is_empty());
    assert!(warnings(&mk(&["doc:pricing"]), &l, &cfg).is_empty());
    // nothing to judge on an empty file list
    assert!(warnings(&mk(&[]), &l, &cfg).is_empty());
}
