//! `chunk apply` (SPEC §4), `approve`.

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
    // v6: the apply's rows share one batch and name their plan; `add`'s rows have neither
    let n = |sql: &str| -> i64 { s.conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(n("SELECT COUNT(DISTINCT batch) FROM event"), 1);
    assert_eq!(n("SELECT COUNT(*) FROM event WHERE batch IS NULL"), 5);
    assert_eq!(n("SELECT COUNT(*) FROM event WHERE plan IS NULL"), 0);
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
fn approve_keeps_the_state() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("c"));
    s.approve(&c, Some((Some("opus"), Some("claude"))), NOW)
        .unwrap();
    let q = "SELECT approved_model || ' ' || approved_client || ' ' || approved_at FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &c).unwrap(), format!("opus claude {NOW}"));
    s.approve(&c, None, NOW).unwrap();
    let q = "SELECT COALESCE(approved_at, approved_model, 'clear') FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &c).unwrap(), "clear");
    assert_eq!(state(&s, &c), "open");
    s.owner(&c, super::super::Owner::Drop, Some("no"), NOW)
        .unwrap();
    assert!(s.approve(&c, None, NOW).unwrap_err().contains("history"));
}

#[test]
fn approve_defaults_to_the_model_hint_and_claude() {
    let mut s = Store::open_in_memory().unwrap();
    let c = add(&mut s, "c", Some("c"));
    let text = s.approve(&c, Some((None, None)), NOW).unwrap();
    assert_eq!(
        text,
        "approved · model small (default — pass --model M to change) · client claude"
    );
    let q = "SELECT approved_model FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &c).as_deref(), Some("small"));
    s.conn
        .execute("UPDATE chunk SET model_hint = 'haiku' WHERE uid = ?1", [&c])
        .unwrap();
    // a re-approve keeps the model already approved, the hint only fills a fresh approval
    let text = s.approve(&c, Some((None, None)), NOW).unwrap();
    assert_eq!(text, "approved · model small · client claude");
    s.approve(&c, None, NOW).unwrap();
    let text = s.approve(&c, Some((None, None)), NOW).unwrap();
    assert_eq!(text, "approved · model haiku · client claude");
    let text = s.approve(&c, Some((Some("sonnet"), None)), NOW).unwrap();
    assert_eq!(text, "approved · model sonnet · client claude");
}

#[test]
fn merge_carries_size_and_checks_it() {
    let mut s = Store::open_in_memory().unwrap();
    let [b, c] = ["b", "c"].map(|t| add(&mut s, t, Some(t)));
    let j = format!(r#"[{{"op":"merge","chunks":["{b}","{c}"],"size":"M"}}]"#);
    let out = ops(&mut s, &j).unwrap();
    let m = out[0].rsplit(' ').next().unwrap().to_string();
    let q = "SELECT COALESCE(size, '-') FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, q, &m).as_deref(), Some("M"));
    let [d] = ["d"].map(|t| add(&mut s, t, Some(t)));
    let j = format!(r#"[{{"op":"merge","chunks":["{d}","{m}"],"size":"huge"}}]"#);
    let e = ops(&mut s, &j).unwrap_err();
    assert!(e.contains("--size huge — S, M or L"), "{e}");
    // a client hint is gone (epoch v8): an op that names one is refused whole
    let j = format!(r#"[{{"op":"merge","chunks":["{d}","{m}"],"client":"claude"}}]"#);
    assert!(serde_json::from_str::<Vec<Op>>(&j).is_err());
}

#[test]
fn merge_and_split_carry_the_issues_the_old_chunks_fix() {
    let mut s = Store::open_in_memory().unwrap();
    let f = |t: &str, issue: &str| super::super::Fields {
        title: Some(t.into()),
        brief: Some(t.into()),
        issues: vec![issue.into()],
        ..Default::default()
    };
    let a = s.add("inbox", &f("a", "I1"), &[], NOW).unwrap();
    let b = s.add("inbox", &f("b", "I2"), &[], NOW).unwrap();
    ops(
        &mut s,
        &format!(r#"[{{"op":"merge","chunks":["{a}","{b}"]}}]"#),
    )
    .unwrap();
    let fix = s.fixers().unwrap();
    let m = fix["I1"].uid.clone();
    assert!(m != a && fix["I2"].uid == m && fix["I1"].state == "open");
    ops(
        &mut s,
        &format!(r#"[{{"op":"split","chunk":"{m}","into":[{{"title":"x"}},{{"title":"y"}}]}}]"#),
    )
    .unwrap();
    let parts = s.board(NOW, NOW).unwrap().chunks;
    let parts: Vec<_> = parts.iter().filter(|c| c.state == "draft").collect();
    assert_eq!(parts.len(), 2);
    for c in parts {
        assert_eq!(c.issues, ["I1", "I2"], "{}", c.title);
    }
}

#[test]
fn merge_and_split_name_the_new_chunks_and_old_labels_stay_history() {
    // issue 01M4QBRX: a merged chunk had no way to take a label
    let mut s = Store::open_in_memory().unwrap();
    let [a, b] = ["a", "b"].map(|t| add(&mut s, t, Some(t)));
    for (u, l) in [(&a, "l3"), (&b, "l4")] {
        s.conn
            .execute("UPDATE chunk SET label = ?1 WHERE uid = ?2", [l, u])
            .unwrap();
    }
    let lbl = |s: &Store, u: &str| get(s, "SELECT label FROM chunk WHERE uid = ?1", u);
    let j = format!(r#"[{{"op":"merge","chunks":["{a}","{b}"],"label":"l3"}}]"#);
    let e = ops(&mut s, &j).unwrap_err();
    assert!(
        e.contains(&format!("label l3 is taken by chunk {a}")),
        "{e}"
    );
    let j = format!(r#"[{{"op":"merge","chunks":["{a}","{b}"],"title":"ab","label":"ledger"}}]"#);
    let out = ops(&mut s, &j).unwrap();
    let m = out[0].rsplit(' ').next().unwrap().to_string();
    assert_eq!(lbl(&s, &m).as_deref(), Some("ledger"));
    assert_eq!(lbl(&s, &a).as_deref(), Some("l3"));
    let j =
        r#"[{"op":"split","chunk":"ledger","into":[{"title":"x","label":"x1"},{"title":"y"}]}]"#;
    let out = ops(&mut s, j).unwrap();
    let parts: Vec<&str> = out[0].rsplit("→ ").next().unwrap().split(", ").collect();
    assert_eq!(lbl(&s, parts[0]).as_deref(), Some("x1"));
    assert_eq!(lbl(&s, parts[1]), None);
}

#[test]
fn merge_unions_reads_and_split_inherits_them_unless_a_part_names_its_own() {
    let mut s = Store::open_in_memory().unwrap();
    let [a, b, c] = ["a", "b", "c"].map(|t| add(&mut s, t, Some(t)));
    let set = |s: &Store, uid: &str, reads: &str| {
        s.conn
            .execute("UPDATE chunk SET reads = ?1 WHERE uid = ?2", [reads, uid])
            .unwrap();
    };
    set(&s, &a, "goal\nx/api");
    set(&s, &b, "x/api\nmodel");
    set(&s, &c, "goal");
    let j = format!(
        r#"[{{"op":"merge","chunks":["{a}","{b}"]}},
            {{"op":"split","chunk":"{c}","into":[{{"title":"c1"}},{{"title":"c2","reads":["done"]}}]}}]"#
    );
    let out = ops(&mut s, &j).unwrap();
    let m = out[0].rsplit(' ').next().unwrap().to_string();
    let reads = "SELECT COALESCE(reads, '-') FROM chunk WHERE uid = ?1";
    assert_eq!(get(&s, reads, &m).as_deref(), Some("goal\nx/api\nmodel"));
    let by_title = "SELECT COALESCE(reads, '-') FROM chunk WHERE title = ?1";
    assert_eq!(get(&s, by_title, "c1").as_deref(), Some("goal"));
    assert_eq!(get(&s, by_title, "c2").as_deref(), Some("done"));
}

#[test]
fn split_carries_label_size_scope_in_one_op() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    s.conn
        .execute(
            "UPDATE chunk SET size = 'M', scope = 'src/', reads = 'goal' WHERE uid = ?1",
            [&a],
        )
        .unwrap();
    let j = format!(
        r#"[{{"op":"split","chunk":"{a}","into":[{{"title":"a1","label":"b1","brief":"one","size":"S","model":"opus","scope":["docs/"],"reads":["model"]}},{{"title":"a2","label":"b2"}}]}}]"#
    );
    let out = ops(&mut s, &j).unwrap();
    assert!(
        !out[0].contains("warning"),
        "explicit scopes differ: {}",
        out[0]
    );
    let q = "SELECT COALESCE(size, '-') FROM chunk WHERE title = ?1";
    assert_eq!(get(&s, q, "a1").as_deref(), Some("S"));
    assert_eq!(get(&s, q, "a2").as_deref(), Some("M"), "size inherits");
    let scope = "SELECT COALESCE(scope, '-') FROM chunk WHERE title = ?1";
    assert_eq!(get(&s, scope, "a1").as_deref(), Some("docs/"));
    assert_eq!(
        get(&s, scope, "a2").as_deref(),
        Some("src/"),
        "scope inherits"
    );
    let label = "SELECT label FROM chunk WHERE title = ?1";
    assert_eq!(get(&s, label, "a1").as_deref(), Some("b1"));
    assert_eq!(get(&s, label, "a2").as_deref(), Some("b2"));
    let brief = "SELECT brief FROM chunk WHERE title = ?1";
    assert_eq!(get(&s, brief, "a1").as_deref(), Some("one"));
}

#[test]
fn scopeless_split_of_scoped_parent_warns() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("a"));
    s.conn
        .execute("UPDATE chunk SET scope = 'src/' WHERE uid = ?1", [&a])
        .unwrap();
    let j =
        format!(r#"[{{"op":"split","chunk":"{a}","into":[{{"title":"a1"}},{{"title":"a2"}}]}}]"#);
    let out = ops(&mut s, &j).unwrap();
    assert!(
        out[0].contains("warning"),
        "inherited overlap warns: {}",
        out[0]
    );
    assert!(out[0].contains("src/"), "{}", out[0]);
    let scope = "SELECT COALESCE(scope, '-') FROM chunk WHERE title = ?1";
    assert_eq!(get(&s, scope, "a1").as_deref(), Some("src/"));
    assert_eq!(get(&s, scope, "a2").as_deref(), Some("src/"));
}
