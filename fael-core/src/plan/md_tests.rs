use super::*;

#[test]
fn labels_read_like_fapony() {
    for (line, want) in [
        ("- [ ] b1 — plan store", Some("b1")),
        ("- [x] chunk 0 — five packages — `fe15b96`", Some("0")),
        ("- [ ] **chunk F3** — web base", Some("F3")),
        ("  - [ ] 3e (wait owner decides) — mark: column", Some("3e")),
        ("- [x] pr0 (4ddb64e, #226) — first", Some("pr0")),
        ("- [ ] chunk-3b — split", Some("3b")),
        ("- [ ] b5b — Thai owner view", Some("b5b")),
        ("- [ ] k8 — after PLAN-vela chunk 6", Some("k8")),
        // no dash after the token, prose after "chunk", too many letters
        ("- [ ] b1 plan store", None),
        ("- [ ] chunks 1-3 — merged", None),
        ("- [ ] abcd1 — x", None),
        ("- [ ] 3c - hyphen is no dash", None),
    ] {
        assert_eq!(label(line).as_deref(), want, "{line}");
    }
}

#[test]
fn markers_and_after() {
    let l = "- [ ] 3f (wip feat/vela-line-bind) — group (after 2, vela-jobs:chunk-j4)";
    assert_eq!(marker(l, "wip").as_deref(), Some("feat/vela-line-bind"));
    assert_eq!(marker(l, "wait"), None);
    assert_eq!(
        after_refs(l),
        Some(vec!["2".to_string(), "vela-jobs:j4".to_string()])
    );
    // a word boundary: (waiting …) is no (wait …); Thai right after is a boundary
    assert_eq!(marker("x (waiting on y)", "wait"), None);
    assert_eq!(marker("x (waitผู้ใช้)", "wait").as_deref(), Some("ผู้ใช้"));
    assert_eq!(marker("x (wip)", "wip").as_deref(), Some(""));
    assert_eq!(after_refs("x (after —)"), Some(vec![]));
    assert_eq!(after_refs("x (after none)"), Some(vec![]));
    assert_eq!(after_refs("x"), None);
    // `(afterward` is no marker; a later real one still counts
    assert_eq!(
        after_refs("(afterwards) (after chunk 3)"),
        Some(vec!["3".to_string()])
    );
}

const PLAN: &str = "---
kind: unit   # tracker
area: marketing
spec: ../spec/SPEC-x.md extra
status: blocked
---

# PLAN-x — the title

> **Status:** started

## TL;DR
- **What:** text
  - [x] a1 — done — abc1234
  - [~] a2 — dropped (fael:01ABC)
  - [ ] a3 (after —) — open
  - [-] a4 — odd box
  - [ ]  x
## 1. Goal
- [ ] g1 — not in the first section
";

#[test]
fn parses_front_title_and_first_section_only() {
    let p = parse("PLAN-X.md", PLAN).unwrap();
    assert_eq!(p.name, "x");
    assert_eq!(p.title, "PLAN-x — the title");
    assert_eq!(p.front.kind.as_deref(), Some("unit"));
    assert_eq!(p.front.area.as_deref(), Some("marketing"));
    assert_eq!(p.front.spec.as_deref(), Some("SPEC-x.md"));
    assert_eq!(p.front.status.as_deref(), Some("blocked"));
    let ticks: Vec<_> = p.items.iter().map(|i| i.tick).collect();
    assert_eq!(
        ticks,
        [
            Tick::Done,
            Tick::Dropped,
            Tick::Open,
            Tick::Unknown,
            Tick::Open
        ]
    );
    // `- [ ] ` with nothing after is fapony's unknown; `- [ ]` alone is no checkbox
    assert_eq!(item("- [ ] ").map(|i| i.tick), Some(Tick::Unknown));
    assert_eq!(item("- [ ]"), None);
    assert_eq!(p.items[2].text, "a3 (after —) — open");
    assert_eq!(p.items[3].text, "- [-] a4 — odd box");
    assert!(parse("SPEC-x.md", PLAN).is_none());
    assert!(parse("PLAN-.md", PLAN).is_none());
}
