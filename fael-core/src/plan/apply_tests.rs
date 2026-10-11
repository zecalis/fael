//! `chunk apply` (SPEC §4), `pin`, `approve`.

use super::super::Store;
use super::super::chunk::tests::{NOW, add, get, start, state};
use super::Op;

fn ops(s: &mut Store, json: &str) -> Result<Vec<String>, String> {
    s.apply(serde_json::from_str::<Vec<Op>>(json).unwrap(), NOW)
}

fn edges(s: &Store, uid: &str, kind: &str) -> Vec<String> {
    s.conn
        .prepare(
            "SELECT d.uid FROM edge e JOIN chunk c ON c.id = e.src JOIN chunk d ON d.id = e.dst
             WHERE c.uid = ?1 AND e.kind = ?2 ORDER BY d.seq",
        )
        .unwrap()
        .query_map([uid, kind], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Plan order of every chunk.
fn order(s: &Store) -> Vec<String> {
    s.conn
        .prepare("SELECT title FROM chunk ORDER BY seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn merge_joins_and_takes_the_old_ones_place() {
    let mut s = Store::open_in_memory().unwrap();
    let [dep, a, b, tail, waits] =
        ["dep", "a", "b", "tail", "waits"].map(|t| add(&mut s, t, Some(t)));
    s.conn
        .execute("UPDATE chunk SET scope = 'x/' WHERE uid = ?1", [&a])
        .unwrap();
    let j = format!(
        r#"[{{"op":"after","chunk":"{a}","on":"{dep}","why":"needs dep"}},
            {{"op":"after","chunk":"{b}","on":"{a}","why":"a first"}},
            {{"op":"after","chunk":"{waits}","on":"{b}","why":"b first"}},
            {{"op":"pair","chunks":["{a}","{tail}"]}},
            {{"op":"merge","chunks":["{a}","{b}"]}}]"#
    );
    let out = ops(&mut s, &j).unwrap();
    let m = out[4].rsplit(' ').next().unwrap().to_string();
    assert_eq!(order(&s), ["dep", "a", "a + b", "b", "tail", "waits"]);
    assert_eq!(
        (state(&s, &a), state(&s, &b), state(&s, &m)),
        ("replaced".into(), "replaced".into(), "open".into())
    );
    assert_eq!(
        get(&s, "SELECT brief FROM chunk WHERE uid = ?1", &m).unwrap(),
        "a\n\nb"
    );
    assert_eq!(
        get(&s, "SELECT scope FROM chunk WHERE uid = ?1", &m).unwrap(),
        "x/"
    );
    // the union of after (minus each other), dependents move over, the pair is kept
    assert_eq!(edges(&s, &m, "after"), std::slice::from_ref(&dep));
    assert_eq!(edges(&s, &waits, "after"), std::slice::from_ref(&m));
    assert_eq!(edges(&s, &m, "pair"), std::slice::from_ref(&tail));
    assert_eq!(edges(&s, &a, "replaced_by"), std::slice::from_ref(&m));
    assert!(edges(&s, &a, "pair").is_empty());
    let ev = get(
        &s,
        "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC",
        &b,
    )
    .unwrap();
    assert_eq!(ev, format!("merged into {m}"));
}

#[test]
fn split_inherits_after_and_one_bad_op_applies_nothing() {
    let mut s = Store::open_in_memory().unwrap();
    let [dep, a, waits] = ["dep", "a", "waits"].map(|t| add(&mut s, t, Some(t)));
    let ok = format!(
        r#"[{{"op":"after","chunk":"{a}","on":"{dep}","why":"w"}},
            {{"op":"after","chunk":"{waits}","on":"{a}","why":"w"}}]"#
    );
    ops(&mut s, &ok).unwrap();
    let bad = format!(
        r#"[{{"op":"split","chunk":"{a}","into":[{{"title":"a1","brief":"one"}},{{"title":"a2"}}]}},
            {{"op":"pair","chunks":["{a}","{dep}"]}}]"#
    );
    let e = ops(&mut s, &bad).unwrap_err();
    assert!(
        e.starts_with("op 2: rejected: chunk")
            && e.contains("replaced")
            && e.ends_with("nothing applied"),
        "{e}"
    );
    assert_eq!(
        (state(&s, &a), order(&s).len()),
        ("open".into(), 3),
        "the split rolled back"
    );
    let good = bad.replace(
        &format!(
            r#",
            {{"op":"pair","chunks":["{a}","{dep}"]}}"#
        ),
        "",
    );
    let out = ops(&mut s, &good).unwrap();
    assert_eq!(order(&s), ["dep", "a", "a1", "a2", "waits"]);
    let parts: Vec<String> = edges(&s, &a, "replaced_by");
    assert_eq!(out[0], format!("split {a} → {}", parts.join(", ")));
    assert_eq!(
        (state(&s, &parts[0]), state(&s, &parts[1])),
        ("open".into(), "draft".into())
    );
    for p in &parts {
        assert_eq!(edges(&s, p, "after"), std::slice::from_ref(&dep));
    }
    assert_eq!(edges(&s, &waits, "after"), parts);
}

#[test]
fn rejects_running_cycles_and_unknown_fields() {
    let mut s = Store::open_in_memory().unwrap();
    let [a, b] = ["a", "b"].map(|t| add(&mut s, t, Some(t)));
    let j = format!(
        r#"[{{"op":"after","chunk":"{a}","on":"{b}","why":"w"}},{{"op":"after","chunk":"{b}","on":"{a}","why":"w"}}]"#
    );
    assert!(ops(&mut s, &j).unwrap_err().contains("closes a loop"));
    assert!(
        edges(&s, &a, "after").is_empty(),
        "the first after rolled back"
    );
    assert!(serde_json::from_str::<Vec<Op>>(r#"[{"op":"merge","chunks":[],"nope":1}]"#).is_err());
    start(&mut s, &a, "/wt").unwrap();
    let j = format!(r#"[{{"op":"merge","chunks":["{a}","{b}"]}}]"#);
    assert!(ops(&mut s, &j).unwrap_err().contains("is running"));
}

#[test]
fn a_split_with_no_gap_renumbers_the_plan() {
    let mut s = Store::open_in_memory().unwrap();
    let [a, _b] = ["a", "b"].map(|t| add(&mut s, t, Some(t)));
    s.conn
        .execute("UPDATE chunk SET seq = seq - 1023 WHERE uid = ?1", [&a])
        .unwrap();
    let j =
        format!(r#"[{{"op":"split","chunk":"{a}","into":[{{"title":"a1"}},{{"title":"a2"}}]}}]"#);
    ops(&mut s, &j).unwrap();
    assert_eq!(order(&s), ["a", "a1", "a2", "b"]);
}

#[test]
fn pin_and_approve_keep_the_state() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("c"));
    s.pin(&c, Some(2), NOW).unwrap();
    s.approve(&c, Some(("opus", "claude")), NOW).unwrap();
    let q = "SELECT pin || ' ' || approved_model || ' ' || approved_client || ' ' || approved_at FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &c).unwrap(), format!("2 opus claude {NOW}"));
    s.pin(&c, None, NOW).unwrap();
    s.approve(&c, None, NOW).unwrap();
    let q = "SELECT COALESCE(pin, approved_at, approved_model, 'clear') FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &c).unwrap(), "clear");
    assert_eq!(state(&s, &c), "open");
    s.owner(&c, super::super::Owner::Drop, Some("no"), NOW)
        .unwrap();
    assert!(s.pin(&c, Some(1), NOW).unwrap_err().contains("history"));
}

#[test]
fn split_and_merge_carry_client_and_check_size() {
    let mut s = Store::open_in_memory().unwrap();
    let [a, b, c] = ["a", "b", "c"].map(|t| add(&mut s, t, Some(t)));
    let j = format!(
        r#"[{{"op":"split","chunk":"{a}","into":[{{"title":"a1","client":"opencode"}},{{"title":"a2"}}]}},
            {{"op":"merge","chunks":["{b}","{c}"],"size":"M","client":"claude"}}]"#
    );
    let out = ops(&mut s, &j).unwrap();
    let m = out[1].rsplit(' ').next().unwrap().to_string();
    let q = "SELECT COALESCE(size, '-') || ' ' || client_hint FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &m).as_deref(), Some("M claude"));
    let hints: Vec<Option<String>> = s
        .conn
        .prepare("SELECT client_hint FROM chunk WHERE title IN ('a1', 'a2') ORDER BY seq")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(hints, [Some("opencode".into()), None]);
    let [d] = ["d"].map(|t| add(&mut s, t, Some(t)));
    let j = format!(r#"[{{"op":"merge","chunks":["{d}","{m}"],"size":"huge"}}]"#);
    let e = ops(&mut s, &j).unwrap_err();
    assert!(e.contains("--size huge — S, M or L"), "{e}");
}
