//! `plan export --since` (decision fael-board:sync-backup): the feed from 0 is everything,
//! from head is nothing, and an edit after head returns the chunk it touched, whole.

use super::super::Fields;
use super::super::Store;
use super::super::chunk::tests::{NOW, add, start};
use super::feed;

#[test]
fn since_head_is_empty_and_an_edit_after_returns_that_chunk_whole() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let b = add(&mut s, "b", Some("do b"));
    start(&mut s, &b, "/wt").unwrap();
    let all = feed(&s, 0).unwrap();
    assert_eq!(all["v"], 1);
    assert_eq!(all["db_id"].as_str().unwrap().len(), 26);
    let head = all["head"].as_i64().unwrap();
    assert!(head > 0);
    assert_eq!(all["plans"].as_array().unwrap().len(), 1);
    assert_eq!(all["chunks"].as_array().unwrap().len(), 2);
    assert_eq!(all["runs"][0]["worktree"], "/wt");

    let none = feed(&s, head).unwrap();
    assert_eq!(
        (
            none["head"].as_i64(),
            none["events"].as_array().unwrap().len()
        ),
        (Some(head), 0)
    );
    assert!(none["chunks"].as_array().unwrap().is_empty());
    assert!(none["plans"].as_array().unwrap().is_empty());

    let f = Fields {
        brief: Some("do a, better".into()),
        ..Fields::default()
    };
    s.edit(&a, &f, NOW).unwrap();
    let next = feed(&s, head).unwrap();
    let chunks = next["chunks"].as_array().unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        (&chunks[0]["uid"], &chunks[0]["brief"]),
        (&a.clone().into(), &"do a, better".into())
    );
    assert!(
        next["runs"].as_array().unwrap().is_empty(),
        "b's run was not touched"
    );
    assert!(next["head"].as_i64().unwrap() > head);
}

#[test]
fn the_feed_carries_every_section_of_a_plan_it_touched() {
    let mut s = Store::open_in_memory().unwrap();
    s.new_plan("", "p", "P", "dev", None, NOW).unwrap();
    let all = feed(&s, 0).unwrap();
    let keys: Vec<&str> = all["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["1", "2", "3", "4"]);
    let head = all["head"].as_i64().unwrap();
    assert!(
        feed(&s, head).unwrap()["sections"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn export_prints_the_label_and_the_notes_the_next_brief_will() {
    let mut s = Store::open_in_memory().unwrap();
    let f = Fields {
        title: Some("t".into()),
        brief: Some("b".into()),
        label: Some("b10".into()),
        ..Fields::default()
    };
    let a = s.add("inbox", &f, &[], NOW).unwrap();
    let plan: i64 = s
        .conn
        .query_row("SELECT plan FROM chunk WHERE uid = ?1", [&a], |r| r.get(0))
        .unwrap();
    s.note(&a, "read before start", NOW).unwrap();
    start(&mut s, &a, "/w").unwrap();
    s.note(&a, "line one\nline two", NOW).unwrap();
    let md = super::export(&s, plan).unwrap();
    assert!(
        md.contains(&format!("  uid {a} · label b10 · running")),
        "{md}"
    );
    assert!(
        md.contains("  > note 2026-10-11: line one line two\n"),
        "{md}"
    );
    assert!(
        !md.contains("read before start"),
        "a note before the last start: {md}"
    );
    assert!(
        super::export_json(&s, plan).unwrap()["chunks"][0]
            .get("notes")
            .is_none()
    );
}

/// The markdown keeps a dropped chunk's reason: without it the export reads
/// bare `dropped` and the reason lives only in the event feed.
#[test]
fn export_prints_the_drop_reason() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    s.owner(&a, super::super::Owner::Drop, Some("out of scope"), NOW)
        .unwrap();
    let plan: i64 = s
        .conn
        .query_row("SELECT plan FROM chunk WHERE uid = ?1", [&a], |r| r.get(0))
        .unwrap();
    let md = super::export(&s, plan).unwrap();
    assert!(
        md.contains("  > dropped 2026-10-11: out of scope\n"),
        "{md}"
    );
}

#[test]
fn an_after_cycle_still_exports_every_chunk_once() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let b = add(&mut s, "b", Some("do b"));
    add(&mut s, "c", Some("do c"));
    // add_after refuses a cycle, so write the edges raw
    for (x, y) in [(&a, &b), (&b, &a)] {
        s.conn
            .execute(
                "INSERT INTO edge(src, dst, kind)
                 SELECT x.id, y.id, 'after' FROM chunk x, chunk y
                 WHERE x.uid = ?1 AND y.uid = ?2",
                [x, y],
            )
            .unwrap();
    }
    let plan: i64 = s
        .conn
        .query_row("SELECT plan FROM chunk WHERE uid = ?1", [&a], |r| r.get(0))
        .unwrap();
    let js = super::export_json(&s, plan).unwrap();
    let mut titles: Vec<&str> = js["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    titles.sort_unstable();
    assert_eq!(titles, ["a", "b", "c"], "{js}");
    assert!(super::export(&s, plan).unwrap().contains("- [ ] b"));
}

#[test]
fn after_edges_order_before_seq_in_markdown_and_json() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let b = add(&mut s, "b", Some("do b"));
    let c = add(&mut s, "c", Some("do c"));
    s.conn
        .execute(
            "INSERT INTO edge(src, dst, kind)
             SELECT x.id, y.id, 'after' FROM chunk x, chunk y
             WHERE x.uid = ?1 AND y.uid = ?2",
            [&b, &c],
        )
        .unwrap();
    let plan: i64 = s
        .conn
        .query_row("SELECT plan FROM chunk WHERE uid = ?1", [&a], |r| r.get(0))
        .unwrap();
    let md = super::export(&s, plan).unwrap();
    let (mut pa, mut pb, mut pc) = (None, None, None);
    for (i, l) in md.lines().enumerate() {
        if l.contains("- [ ] a") {
            pa = Some(i);
        }
        if l.contains("- [ ] b") {
            pb = Some(i);
        }
        if l.contains("- [ ] c") {
            pc = Some(i);
        }
    }
    let (pa, pb, pc) = (pa.unwrap(), pb.unwrap(), pc.unwrap());
    assert!(pa < pc && pc < pb, "{md}");
    let js = super::export_json(&s, plan).unwrap();
    let titles: Vec<&str> = js["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["a", "c", "b"], "{js}");
}
