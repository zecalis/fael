//! `plan apply` (decision fael-board:plan-bill): stale rev, a missing line kept, a running
//! line rejected, dry-run writes nothing, one batch.

use super::super::Store;
use super::super::chunk::tests::{NOW, add, start, state};
use super::super::export_json;
use super::Bill;
use serde_json::{Value, json};

fn export(s: &Store) -> Value {
    export_json(s, s.plans().unwrap()[0].id).unwrap()
}

fn apply(s: &mut Store, v: Value, dry: bool) -> Result<Vec<String>, String> {
    s.bill(serde_json::from_value::<Bill>(v).unwrap(), NOW, dry)
        .map(|(lines, _)| lines)
}

/// The inbox bill with `edit` merged over its header and `chunks` as its lines.
fn bill(s: &Store, edit: Value, chunks: Value) -> Value {
    let mut b = export(s);
    b["chunks"] = chunks;
    for (k, v) in edit.as_object().unwrap() {
        b[k] = v.clone();
    }
    b
}

fn count(s: &Store, sql: &str) -> i64 {
    s.conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn export_is_the_bill_and_an_unedited_bill_changes_nothing() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let b = add(&mut s, "b", None);
    s.conn
        .execute("UPDATE chunk SET scope = 'x/\ny/' WHERE uid = ?1", [&a])
        .unwrap();
    let e = export(&s);
    assert_eq!(
        (e["v"].as_i64(), e["plan"].as_str()),
        (Some(1), Some("inbox"))
    );
    assert!(e["rev"].as_i64().unwrap() > 0);
    let lines = e["chunks"].as_array().unwrap();
    assert_eq!(lines[0]["uid"], a);
    assert_eq!(lines[0]["scope"], json!(["x/", "y/"]));
    assert_eq!(lines[1]["uid"], b);
    assert_eq!(lines[1]["brief"], "");
    // the export read back: no lines written, rev unmoved
    assert_eq!(
        apply(&mut s, e.clone(), false).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(export(&s), e);
}

#[test]
fn a_stale_rev_rejects_the_whole_bill_and_writes_nothing() {
    let mut s = Store::open_in_memory().unwrap();
    add(&mut s, "a", Some("do a"));
    let old = export(&s);
    add(&mut s, "b", Some("do b"));
    let n = count(&s, "SELECT COUNT(*) FROM event");
    let v = bill(
        &s,
        json!({"rev": old["rev"], "title": "renamed"}),
        json!([]),
    );
    let e = apply(&mut s, v, false).unwrap_err();
    assert!(
        e.contains("stale rev") && e.contains("export inbox --json"),
        "{e}"
    );
    assert_eq!(count(&s, "SELECT COUNT(*) FROM event"), n);
    assert_eq!(export(&s)["title"], "inbox");
}

#[test]
fn lines_add_edit_and_drop_and_a_missing_line_is_left_alone() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let b = add(&mut s, "b", Some("do b"));
    let c = add(&mut s, "c", Some("do c"));
    s.approve(&a, Some(("opus", "claude")), NOW).unwrap();
    let v = bill(
        &s,
        json!({"title": "Inbox!", "doc": "## 1. Goal\nship\n", "refs": ["SPEC.md"]}),
        json!([
            {"uid": a, "brief": "do a better", "after": [b]},
            {"uid": c, "drop": true},
            {"title": "d", "brief": "do d", "label": "d1", "after": [b]},
            {"title": "e"},
        ]),
    );
    let out = apply(&mut s, v, false).unwrap();
    assert_eq!(out.len(), 5, "{out:?}");
    let e = export(&s);
    assert_eq!(
        (e["title"].as_str(), e["doc"].as_str()),
        (Some("Inbox!"), Some("## 1. Goal\nship\n"))
    );
    assert_eq!(e["refs"], json!(["SPEC.md"]));
    let lines = e["chunks"].as_array().unwrap();
    let titles: Vec<&str> = lines.iter().map(|l| l["title"].as_str().unwrap()).collect();
    // b was not in the bill: still there; c dropped: out of the export
    assert_eq!(titles, ["a", "b", "d", "e"]);
    assert_eq!(lines[0]["brief"], "do a better");
    assert_eq!(lines[0]["after"], json!([b]));
    assert_eq!(state(&s, &c), "dropped");
    assert_eq!(lines[2]["label"], "d1");
    assert_eq!(lines[2]["after"], json!([b]));
    assert_eq!(state(&s, lines[2]["uid"].as_str().unwrap()), "open");
    assert_eq!(state(&s, lines[3]["uid"].as_str().unwrap()), "draft");
    // a brief change cleared a's approval
    let ok = count(
        &s,
        "SELECT COUNT(*) FROM chunk WHERE title = 'a' AND approved_at IS NULL",
    );
    assert_eq!(ok, 1);
    // every row of the bill shares one batch
    assert_eq!(
        count(
            &s,
            "SELECT COUNT(DISTINCT batch) FROM event WHERE batch IS NOT NULL"
        ),
        1
    );
    assert_eq!(
        count(
            &s,
            "SELECT COUNT(*) FROM event WHERE batch IS NULL AND id > 4"
        ),
        0
    );
    // the header edit moved the rev too
    assert!(e["rev"].as_i64().unwrap() > count(&s, "SELECT MIN(id) FROM event"));
}

#[test]
fn a_line_that_changes_a_running_or_finished_chunk_rejects_the_bill() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let b = add(&mut s, "b", Some("do b"));
    start(&mut s, &a, "/wt").unwrap();
    s.conn
        .execute("UPDATE chunk SET state = 'done' WHERE uid = ?1", [&b])
        .unwrap();
    let n = count(&s, "SELECT COUNT(*) FROM event");
    // a header edit and an add come before the bad line: none of it stays
    for line in [
        json!({"uid": a, "title": "a2"}),
        json!({"uid": a, "drop": true}),
        json!({"uid": b, "title": "b2"}),
    ] {
        let v = bill(&s, json!({"title": "T"}), json!([{"title": "new"}, line]));
        let e = apply(&mut s, v, false).unwrap_err();
        assert!(e.contains("is running") || e.contains("is done"), "{e}");
    }
    assert_eq!(count(&s, "SELECT COUNT(*) FROM event"), n);
    assert_eq!(export(&s)["title"], "inbox");
    // the same line unchanged is no change: a running chunk can ride in the bill
    let same = export(&s)["chunks"].clone();
    assert!(
        {
            let v = bill(&s, json!({}), same);
            apply(&mut s, v, false)
        }
        .unwrap()
        .is_empty()
    );
}

#[test]
fn dry_run_says_what_it_would_write_and_writes_nothing() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let before = export(&s);
    let v = bill(
        &s,
        json!({"title": "T"}),
        json!([{"uid": a, "title": "a2"}, {"title": "n"}]),
    );
    let out = apply(&mut s, v, true).unwrap();
    assert_eq!(out.len(), 3, "{out:?}");
    assert!(out.iter().any(|l| l.starts_with("+ ")), "{out:?}");
    assert_eq!(export(&s), before);
    assert_eq!(count(&s, "SELECT COUNT(*) FROM chunk"), 1);
}

#[test]
fn ops_ride_in_the_bill_and_must_stay_in_the_plan() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let b = add(&mut s, "b", Some("do b"));
    let v = bill(&s, json!({}), json!([]));
    let mut v2 = v.clone();
    v2["ops"] = json!([{"op": "pair", "chunks": [a, b]}]);
    let out = apply(&mut s, v2, false).unwrap();
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(
        count(&s, "SELECT COUNT(*) FROM edge WHERE kind = 'pair'"),
        1
    );
    // a bad op fails the bill
    let mut v3 = export(&s);
    v3["ops"] = json!([{"op": "pair", "chunks": [a, "nope"]}]);
    let e = apply(&mut s, v3, false).unwrap_err();
    assert!(e.starts_with("op 1:"), "{e}");
}

#[test]
fn a_bill_with_a_bad_header_or_version_is_refused() {
    let mut s = Store::open_in_memory().unwrap();
    add(&mut s, "a", Some("do a"));
    for (edit, want) in [
        (json!({"v": 2}), "v 2"),
        (json!({"area": "ops"}), "dev or marketing"),
        (json!({"state": "done"}), "fael plan park|unpark|done"),
        (json!({"title": " "}), "needs a title"),
        (json!({"plan": "nope"}), "no plan"),
    ] {
        let v = bill(&s, edit, json!([]));
        let e = apply(&mut s, v, false).unwrap_err();
        assert!(e.contains(want), "{want} in {e}");
    }
}
