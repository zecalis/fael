//! title + body — lists show titles, bodies are pulled by id.

use super::{log, row};
use fael_core::*;

fn titled(id: &str, title: &str, text: &str) -> Row {
    Row {
        id: id.into(),
        ts: "2026-09-20T00:00:00Z".into(),
        kind: "decision".into(),
        title: Some(title.into()),
        text: text.into(),
        files: vec!["src/a.rs".into()],
        ..Row::default()
    }
}

#[test]
fn display_title_prefers_title_then_first_sentence() {
    // the title wins verbatim, whitespace collapsed
    let r = titled("T1", "  urgent  is an\norderable number ", "body");
    assert_eq!(r.display_title(), "urgent is an orderable number");
    // no title, short single sentence → unchanged, no ellipsis
    let r = row("T2", "note", &["x"], None);
    assert_eq!(r.display_title(), "text of T2");
    // more sentences → the first one + …
    let mut r = row("T3", "note", &["x"], None);
    r.text = "Fixed the loop. See the job log for details.".into();
    assert_eq!(r.display_title(), "Fixed the loop. …");
    // a dotted path is not a sentence end — only `.` + space/end splits
    let mut r = row("T4", "note", &["x"], None);
    r.text = "see src/a.rs for the full story here".into();
    assert_eq!(r.display_title(), "see src/a.rs for the full story here");
    // long single sentence → 20 words + …
    let mut r = row("T5", "note", &["x"], None);
    r.text = (1..=25)
        .map(|i| format!("w{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(
        r.display_title(),
        format!(
            "{} …",
            (1..=20)
                .map(|i| format!("w{i}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    );
}

#[test]
fn render_lists_titles_full_shows_bodies() {
    let mut l = log();
    l.rows.push(titled(
        "C0000000000000000000000016",
        "headline here",
        "the long body nobody skims",
    ));
    let out = render(&l, &find(&l, &super::files(&["src/a.rs"])), 10_000);
    assert!(out.contains("headline here → src/a.rs"), "{out}");
    assert!(!out.contains("the long body"), "{out}");
    let out = render_full(&l, &find(&l, &super::files(&["src/a.rs"])), 10_000);
    assert!(out.contains("headline here → src/a.rs"), "{out}");
    assert!(out.contains("the long body nobody skims"), "{out}");
}

#[test]
fn long_untitled_text_warns_short_or_titled_does_not() {
    let l = log();
    let cfg = Config::default();
    let long = (1..=61).map(|_| "word").collect::<Vec<_>>().join(" ");
    let mut r = row("D0000000000000000000000017", "note", &["x"], None);
    r.text = long.clone();
    let w = warnings(&r, &l, &cfg);
    assert!(w.iter().any(|x| x.contains("--title")), "{w:?}");
    r.title = Some("headline".into());
    assert!(warnings(&r, &l, &cfg).is_empty());
    r.title = Some((1..=16).map(|_| "word").collect::<Vec<_>>().join(" "));
    let w = warnings(&r, &l, &cfg);
    assert!(w.iter().any(|x| x.contains("aim ≤ 15")), "{w:?}");
}

#[test]
fn long_row_warns_to_split_and_unspaced_text_needs_a_title() {
    let l = log();
    let cfg = Config::default();
    let mut r = row("D0000000000000000000000017", "decision", &["x"], None);
    // ~150 tokens of English: under the token budget, over the char cap
    r.text = "stack pick; ".repeat(55);
    r.title = Some("headline".into());
    let w = warnings(&r, &l, &cfg);
    assert!(w.iter().any(|x| x.contains("one topic per row")), "{w:?}");
    // Thai: no spaces, so one "word" — chars still ask for a title
    r.text = "ก".repeat(450);
    r.title = None;
    let w = warnings(&r, &l, &cfg);
    assert!(w.iter().any(|x| x.contains("--title")), "{w:?}");
    assert!(w.iter().any(|x| x.contains("one topic per row")), "{w:?}");
}

#[test]
fn auto_title_cuts_at_topic_separator_newline_and_80_chars() {
    let mut r = row("D0000000000000000000000017", "decision", &["x"], None);
    // `;`/`·`/`—` join topics the way `.` joins sentences: first topic only
    r.text = "a; b; c".into();
    assert_eq!(r.display_title(), "a …");
    r.text = "ใช้ ก · ใช้ ข · ใช้ ค".into();
    assert_eq!(r.display_title(), "ใช้ ก …");
    r.text = "จบแล้ว — รายละเอียดตามมา".into();
    assert_eq!(r.display_title(), "จบแล้ว …");
    // the second line never leaks into the title, however short the first is
    r.text = "หัวข้อ\nรายละเอียดยาว".into();
    assert_eq!(r.display_title(), "หัวข้อ …");
    // Thai has no `.` or spaces to cut on: 300 chars → ~80 chars + …
    r.text = "ก".repeat(300);
    let t = r.display_title();
    assert!(t.ends_with(" …"), "{t}");
    assert!(t.chars().count() <= 82, "{t}");
    // an explicit title stays verbatim, however long
    r.title = Some("ก".repeat(300));
    assert_eq!(r.display_title().chars().count(), 300);
    // a leading separator with nothing before it falls back to the full head
    r.title = None;
    r.text = "; foo".into();
    assert_eq!(r.display_title(), "; foo");
}

#[test]
fn find_text_matches_titles() {
    let mut l = log();
    l.rows.push(titled(
        "C0000000000000000000000016",
        "queued refunds",
        "zzz",
    ));
    let f = Filter {
        text: Some("QUEUED".into()),
        ..Filter::default()
    };
    assert_eq!(
        find(&l, &f)
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["C0000000000000000000000016"]
    );
}

#[test]
fn bump_keeps_the_title() {
    let dir = std::env::temp_dir().join(format!("fael-bump-title-{}", ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let (cfg, st) = (
        Config::default(),
        Stamp {
            by: "tester-0000".into(),
            branch: None,
            sha: None,
        },
    );
    let mut r = Row::new("tester-0000", "issue", "hot body", vec!["src/a.rs".into()]);
    r.title = Some("hot headline".into());
    let r = add_row(&dir, None, &read(&dir), &cfg, &st, r, None)
        .unwrap()
        .0;
    let (b, _, _) = bump_row(
        &dir,
        None,
        &read(&dir),
        &cfg,
        &st,
        &r.id,
        BumpOpts {
            held: None,
            to: Some("ploy".into()),
            urgent: UrgentChange::Keep,
            revisit: None,
        },
    )
    .unwrap();
    assert_eq!(b.title.as_deref(), Some("hot headline"));
    assert_eq!(b.to.as_deref(), Some("ploy"));
}
