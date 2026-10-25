//! Sections (plan fael-v8): keys, the bill round trip by key, rev, drop, export order and
//! dates, one section alone, and `plan.doc` carried over once.

use super::super::chunk::tests::NOW;
use super::super::{Bill, Store, export, export_json, export_section};
use super::{carry, key_of, list, split};
use serde_json::{Value, json};

const LATER: &str = "2026-10-12T10:00:00Z";

/// The day a section stamped `at` prints: the machine's local day, as `list` dates it.
fn day(at: &str) -> String {
    rusqlite::Connection::open_in_memory()
        .unwrap()
        .query_row("SELECT date(?1, 'localtime')", [at], |r| r.get(0))
        .unwrap()
}

fn plan() -> (Store, i64) {
    let mut s = Store::open_in_memory().unwrap();
    s.new_plan("", "p", "P", "dev", None, NOW).unwrap();
    let id = s.plans().unwrap()[0].id;
    (s, id)
}

fn apply(s: &mut Store, b: &Value, at: &str) -> Result<Vec<String>, String> {
    s.bill(
        serde_json::from_value::<Bill>(b.clone()).unwrap(),
        at,
        false,
    )
    .map(|(l, _)| l)
}

fn with(s: &Store, id: i64, sections: Value) -> Value {
    let mut b = export_json(s, id).unwrap();
    b["sections"] = sections;
    b
}

fn keys(s: &Store, id: i64) -> Vec<String> {
    list(&s.conn, id)
        .unwrap()
        .into_iter()
        .map(|r| r.key)
        .collect()
}

#[test]
fn a_heading_keys_on_its_number_else_its_slug() {
    assert_eq!(key_of("1. Goal (why)"), "1");
    assert_eq!(key_of("§3 Risks"), "§3");
    assert_eq!(key_of("chunk rules"), "chunk-rules");
    let s = split("lead\n\n## 1. Goal\nship\n\n## Notes\na\n## Notes\nb\n");
    let k: Vec<(&str, &str, &str)> = s
        .iter()
        .map(|(k, t, b)| (k.as_str(), t.as_str(), b.as_str()))
        .collect();
    assert_eq!(
        k,
        [
            ("intro", "Intro", "lead"),
            ("1", "1. Goal", "ship"),
            ("notes", "Notes", "a"),
            ("notes-2", "Notes", "b"),
        ]
    );
}

#[test]
fn new_plan_skeleton_points_goal_and_done_at_the_fill_command() {
    let (s, id) = plan();
    let secs = list(&s.conn, id).unwrap();
    for r in secs.iter().filter(|r| r.key == "1" || r.key == "3") {
        assert!(
            r.body.contains("`fael plan export <plan> --json`")
                && r.body.contains("`fael plan apply`"),
            "{r:?}"
        );
        // still blank: the brief prints the fill pointer, not the body
        assert!(super::blank(r), "{r:?}");
    }
}

#[test]
fn a_bill_adds_edits_and_drops_sections_by_key_and_leaves_the_rest() {
    let (mut s, id) = plan();
    assert_eq!(keys(&s, id), ["1", "2", "3", "4"]);
    // the export read back changes nothing
    let e = export_json(&s, id).unwrap();
    assert_eq!(e["sections"][1]["body"], "**Do:**\n**Don't:**");
    assert!(apply(&mut s, &e, LATER).unwrap().is_empty());
    let b = with(
        &s,
        id,
        json!([
            {"key": "1", "body": "ship it"},
            {"key": "§5", "title": "§5 Spec", "body": "the api"},
            {"key": "3", "drop": true},
            {"key": "9", "drop": true},
        ]),
    );
    let out = apply(&mut s, &b, LATER).unwrap();
    assert_eq!(out, ["§ 1 edited", "§ §5 added", "§ 3 dropped"]);
    let e = export_json(&s, id).unwrap();
    assert_eq!(
        e["sections"],
        json!([
            {"key": "1", "title": "1. Goal (why)", "body": "ship it"},
            {"key": "2", "title": "2. Scope", "body": "**Do:**\n**Don't:**"},
            {"key": "4", "title": "4. Constraints", "body": ""},
            {"key": "§5", "title": "§5 Spec", "body": "the api"},
        ])
    );
    let texts: Vec<String> = s
        .conn
        .prepare("SELECT text FROM event WHERE at = ?1 ORDER BY id")
        .unwrap()
        .query_map([LATER], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        texts,
        [
            "1: 1. Goal (why)",
            "§5: §5 Spec",
            "3: 3. Done criteria — dropped"
        ]
    );
    // a new key needs a title, and a key twice is refused: nothing written
    for bad in [
        json!([{"key": "1", "body": "x"}, {"key": "6", "body": "no title"}]),
        json!([{"key": "1", "body": "x"}, {"key": "1", "body": "y"}]),
    ] {
        let b = with(&s, id, bad);
        assert!(
            apply(&mut s, &b, LATER)
                .unwrap_err()
                .starts_with("rejected")
        );
    }
    assert_eq!(export_json(&s, id).unwrap()["sections"], e["sections"]);
}

#[test]
fn a_section_edit_moves_the_rev_so_an_older_bill_is_stale() {
    let (mut s, id) = plan();
    let old = export_json(&s, id).unwrap();
    let b = with(&s, id, json!([{"key": "2", "body": "only cli"}]));
    apply(&mut s, &b, LATER).unwrap();
    assert!(export_json(&s, id).unwrap()["rev"].as_i64() > old["rev"].as_i64());
    let mut stale = old;
    stale["sections"] = json!([{"key": "2", "body": "everything"}]);
    assert!(
        apply(&mut s, &stale, LATER)
            .unwrap_err()
            .contains("stale rev")
    );
    assert_eq!(
        export_json(&s, id).unwrap()["sections"][1]["body"],
        "only cli"
    );
}

#[test]
fn export_prints_sections_in_order_each_dated_and_one_alone() {
    let (mut s, id) = plan();
    let b = with(&s, id, json!([{"key": "2", "body": "only cli"}]));
    apply(&mut s, &b, LATER).unwrap();
    let md = export(&s, id).unwrap();
    let (first, later) = (day(NOW), day(LATER));
    let goal = md
        .find(&format!(
            "## 1. Goal (why)\n> section 1 · updated {first}\n"
        ))
        .unwrap();
    let scope = md
        .find(&format!(
            "## 2. Scope\n> section 2 · updated {later}\n\nonly cli\n"
        ))
        .unwrap();
    let done = md.find("## 3. Done criteria\n").unwrap();
    assert!(goal < scope && scope < done, "{md}");
    assert_eq!(
        export_section(&s, id, "2").unwrap(),
        format!("## 2. Scope\n> section 2 · updated {later}\n\nonly cli\n")
    );
    let e = export_section(&s, id, "9").unwrap_err();
    assert!(
        e.contains("no section 9") && e.contains("1, 2, 3, 4"),
        "{e}"
    );
}

#[test]
fn a_section_resolves_by_key_title_or_stripped_title() {
    use super::resolve;
    let (mut s, id) = plan();
    let b = with(
        &s,
        id,
        json!([{"key": "§5", "title": "§5 Risks", "body": "none said"}]),
    );
    apply(&mut s, &b, LATER).unwrap();
    let secs = list(&s.conn, id).unwrap();
    // exact key still wins
    assert_eq!(resolve(&secs, "1").unwrap().key, "1");
    // title, stripped title and case/dash folding
    assert_eq!(resolve(&secs, "1. Goal (why)").unwrap().key, "1");
    assert_eq!(resolve(&secs, "goal (why)").unwrap().key, "1");
    assert_eq!(resolve(&secs, "GOAL (WHY)").unwrap().key, "1");
    assert_eq!(resolve(&secs, "§5 Risks").unwrap().key, "§5");
    assert_eq!(resolve(&secs, "risks").unwrap().key, "§5");
    assert_eq!(resolve(&secs, "§5").unwrap().key, "§5");
    // export --section takes them too, and a miss still lists the keys
    assert!(
        export_section(&s, id, "Goal (why)")
            .unwrap()
            .contains("## 1. Goal")
    );
    assert!(
        export_section(&s, id, "risks")
            .unwrap()
            .contains("## §5 Risks")
    );
    let e = export_section(&s, id, "9").unwrap_err();
    assert!(
        e.contains("no section 9") && e.contains("1, 2, 3, 4"),
        "{e}"
    );
    // ambiguous: two sections share the stripped title — no guess, keys listed
    let b = with(
        &s,
        id,
        json!([
            {"key": "n1", "title": "Notes", "body": "a"},
            {"key": "n2", "title": "NOTES", "body": "b"},
        ]),
    );
    apply(&mut s, &b, LATER).unwrap();
    let secs = list(&s.conn, id).unwrap();
    assert!(resolve(&secs, "notes").is_none());
    let e = export_section(&s, id, "notes").unwrap_err();
    assert!(e.contains("no section notes"), "{e}");
}

#[test]
fn a_plan_doc_is_cut_into_sections_once() {
    let (s, id) = plan();
    let tx = s.conn.unchecked_transaction().unwrap();
    // `plan.doc` as a db before epoch v8 had it
    tx.execute_batch(
        "DELETE FROM section;
         ALTER TABLE plan ADD COLUMN doc TEXT;
         UPDATE plan SET doc = '## 1. Goal (why)
ship

## 2. Scope
only cli
'",
    )
    .unwrap();
    carry(&tx).unwrap();
    tx.commit().unwrap();
    let rows = list(&s.conn, id).unwrap();
    let got: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.key.as_str(), r.body.as_str()))
        .collect();
    assert_eq!(got, [("1", "ship"), ("2", "only cli")]);
    assert!(rows.iter().all(|r| !r.updated.is_empty()));
}

#[test]
fn a_chunk_reads_its_sections_and_another_plans_and_the_brief_indexes_the_rest() {
    use super::super::{Fields, Started};
    let (mut s, p) = plan();
    s.new_plan("", "q", "Q", "dev", None, NOW).unwrap();
    let q = s
        .plans()
        .unwrap()
        .iter()
        .find(|r| r.name == "q")
        .unwrap()
        .id;
    // every base section says something: one left as `plan new` wrote it is never printed
    let sec = json!([{"key": "1", "body": "ship p"}, {"key": "2", "body": "cli only"},
        {"key": "3", "body": "tests pass"}, {"key": "4", "body": "400 lines"},
        {"key": "§5", "title": "§5 Risks", "body": "none said"}]);
    let b = with(&s, p, sec);
    apply(&mut s, &b, LATER).unwrap();
    let sec = json!([{"key": "§1", "title": "§1 Access", "body": "who sees what"}]);
    let b = with(&s, q, sec);
    apply(&mut s, &b, LATER).unwrap();
    let f = |reads: Option<&[&str]>| Fields {
        title: Some("t".into()),
        brief: Some("b".into()),
        reads: reads.map(|r| r.iter().map(|x| x.to_string()).collect()),
        ..Fields::default()
    };
    let reads = ["§5", "q/§1", "p/1", "gone", "q/nope", "x/1"];
    let uid = s.add("p", &f(Some(&reads)), &[], NOW).unwrap();
    let text = |s: &Store, uid: &str| {
        Started {
            run: "R".into(),
            chunks: vec![s.show(uid).unwrap()],
            orphaned: vec![],
        }
        .text()
    };
    let t = text(&s, &uid);
    // Goal and Done, then what it reads (its own plan's by key, another's by plan/key)
    let later = day(LATER);
    assert!(
        t.contains(&format!(
            "<!-- plan p -->\n\n## 1. Goal (why)\n> section 1 · updated {later}\n\nship p\n"
        )) && t.contains("\n## 3. Done criteria\n")
            && t.contains(&format!(
                "\n## §5 Risks\n> section §5 · updated {later}\n\nnone said\n"
            ))
            && t.contains(&format!(
                "\n## §1 Access\n> section q/§1 · updated {later}\n\nwho sees what\n"
            )),
        "{t}"
    );
    assert_eq!(
        t.matches("## 1. Goal").count(),
        1,
        "p/1 is Goal, printed once: {t}"
    );
    for miss in ["gone", "q/nope", "x/1"] {
        assert!(t.contains(&format!("\nsection {miss} not found\n")), "{t}");
    }
    let day = day(LATER);
    assert!(
        !t.contains("## 2. Scope")
            && t.contains(&format!(
                "\nother sections of p — read one: `fael plan export p --section <key>`\n\
                 - 2 · 2. Scope · {day}\n- 4 · 4. Constraints · {day}\n"
            )),
        "{t}"
    );
    assert!(
        t.contains("a section you read that your change makes untrue"),
        "{t}"
    );
    // the bill reads them back
    let b = export_json(&s, p).unwrap();
    assert_eq!(b["chunks"][0]["reads"], json!(reads));
    // no reads: Goal, Scope, Done, Constraints, the rest indexed
    let plain = s.add("p", &f(None), &[], NOW).unwrap();
    let t = text(&s, &plain);
    assert!(
        t.contains("\n## 2. Scope\n")
            && t.contains("\n## 4. Constraints\n")
            && t.contains(&format!("\n- §5 · §5 Risks · {later}\n"))
            && !t.contains("## §5")
            && !t.contains("not found"),
        "{t}"
    );
    // edit sets them; a ref is one word
    s.edit(
        &plain,
        &Fields {
            reads: Some(vec!["§5".into()]),
            ..Fields::default()
        },
        NOW,
    )
    .unwrap();
    assert!(text(&s, &plain).contains("\n## §5 Risks\n"));
    let bad = Fields {
        reads: Some(vec!["a b".into()]),
        ..Fields::default()
    };
    assert!(
        s.edit(&plain, &bad, NOW)
            .unwrap_err()
            .starts_with("rejected: reads")
    );
}
