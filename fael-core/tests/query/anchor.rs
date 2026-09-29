//! kickoff widening — `<PREFIX><name>.md` filters also match `<prefix>:<name>` anchors.

use super::{files, ids, row};
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
