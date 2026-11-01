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
    s.approve(&a, Some((Some("opus"), Some("claude"))), NOW)
        .unwrap();
    let v = bill(
        &s,
        json!({"title": "Inbox!",
               "sections": [{"key": "1", "title": "1. Goal", "body": "ship"}]}),
        json!([
            {"uid": a, "brief": "do a better", "after": [b]},
            {"uid": c, "drop": true},
            {"title": "d", "brief": "do d", "label": "d1", "after": [b]},
            {"title": "e"},
        ]),
    );
    let out = apply(&mut s, v, false).unwrap();
    assert_eq!(out.len(), 6, "{out:?}");
    let e = export(&s);
    assert_eq!(e["title"], "Inbox!");
    assert_eq!(
        e["sections"],
        json!([{"key": "1", "title": "1. Goal", "body": "ship"}])
    );
    assert!(e.get("refs").is_none() && e.get("repo").is_none(), "{e}");
    let lines = e["chunks"].as_array().unwrap();
    let titles: Vec<&str> = lines.iter().map(|l| l["title"].as_str().unwrap()).collect();
    // b was not in the bill: still there; c dropped: out of the export.
    // a waits on b, so the export orders b first (after edges before seq).
    assert_eq!(titles, ["b", "a", "d", "e"]);
    assert_eq!(lines[1]["brief"], "do a better");
    assert_eq!(lines[1]["after"], json!([b]));
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

#[test]
fn a_bill_wait_gates_start_until_the_owner_answers_and_round_trips() {
    use super::super::Owner;
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let w = json!({"on": "owner", "ask": "real job round first?", "options": ["yes", "no"]});
    let mut line = json!({"uid": a, "wait": w});
    let v = bill(&s, json!({}), json!([line.clone()]));
    let out = apply(&mut s, v, false).unwrap();
    assert!(out[0].contains("wait owner"), "{out:?}");
    assert_eq!(state(&s, &a), "waiting");
    let e = start(&mut s, &a, "/w").unwrap_err();
    assert!(
        e.contains("it waits on owner") && e.contains("fael chunk answer"),
        "{e}"
    );
    // exported back: the same wait is no change
    let ex = export(&s);
    assert_eq!(ex["chunks"][0]["wait"], w);
    assert!(apply(&mut s, ex, false).unwrap().is_empty());
    line["wait"]["ask"] = json!("changed?");
    let v = bill(&s, json!({}), json!([line]));
    assert_eq!(apply(&mut s, v, false).unwrap().len(), 1);
    s.owner(&a, Owner::Answer, Some("yes"), NOW).unwrap();
    assert_eq!(state(&s, &a), "open");
    start(&mut s, &a, "/w").unwrap();
}

#[test]
fn a_bill_adds_a_gated_chunk_and_refuses_a_bad_or_running_wait() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("do a"));
    let v = bill(
        &s,
        json!({}),
        json!([{"title": "g", "brief": "b", "wait": {"on": "data", "ask": "n>=5", "until": "2026-12-01"}}]),
    );
    assert!(apply(&mut s, v, false).unwrap()[0].contains("(waiting)"));
    let n = count(
        &s,
        "SELECT COUNT(*) FROM chunk WHERE state = 'waiting' AND wait_until = '2026-12-01'",
    );
    assert_eq!(n, 1);
    for (w, want) in [
        (json!({"on": "data", "ask": "q"}), "needs wait.until"),
        (
            json!({"on": "owner", "ask": "q", "until": "soon"}),
            "YYYY-MM-DD",
        ),
        (
            json!({"on": "data", "ask": "q", "until": "2026-12-01", "options": ["x"]}),
            "options needs",
        ),
        (json!({"on": "me", "ask": "q"}), "owner or data"),
        (json!({"on": "owner", "ask": " "}), "needs an ask"),
    ] {
        let v = bill(&s, json!({}), json!([{"uid": a, "wait": w}]));
        let e = apply(&mut s, v, false).unwrap_err();
        assert!(e.contains(want), "{want} in {e}");
    }
    start(&mut s, &a, "/w").unwrap();
    let v = bill(
        &s,
        json!({}),
        json!([{"uid": a, "wait": {"on": "owner", "ask": "q"}}]),
    );
    assert!(
        apply(&mut s, v, false)
            .unwrap_err()
            .contains("changes nothing that runs")
    );
}
