use super::*;
use crate::stats::parse::parse;
use std::path::{Path, PathBuf};

/// A usage line of `session` at minute `min` (a minute may pass 59: hours roll).
fn line(session: &str, min: u32, rest: &str) -> String {
    format!(
        "{{\"ts\":\"2026-10-05T{:02}:{:02}:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"{session}\",{rest}}}\n",
        min / 60,
        min % 60
    )
}

/// A search push that said `id` with `touch` of its files already touched.
fn push(session: &str, min: u32, id: &str, touch: u32, extra: &str) -> String {
    line(
        session,
        min,
        &format!(
            "\"event\":\"search\",\"trigger\":\"hitlist\",\"files\":[\"a.rs\"],\"ids\":[\"{id}\"],\"feat\":{{\"{id}\":{{\"tier\":0,\"hub\":false,\"kind\":\"decision\",\"age_d\":1,\"touch\":{touch}}}}}{extra}"
        ),
    )
}

fn cite(session: &str, min: u32, id: &str) -> String {
    line(
        session,
        min,
        &format!("\"event\":\"outcome\",\"ids\":[],\"cited\":[\"{id}\"]"),
    )
}

fn pull(session: &str, min: u32, q: &str, id: &str) -> String {
    line(
        session,
        min,
        &format!("\"event\":\"find\",\"found\":[\"{id}\"],\"q\":{q}"),
    )
}

fn run(usage: &str, kinds: &[(&str, &str)]) -> Tune {
    let row = |(id, kind): &(&str, &str)| {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-04T00:00:00Z\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"a.rs\"]}}\n"
        )
    };
    let log = Log {
        rows: crate::stats::said::tests::rows(&kinds.iter().map(row).collect::<String>()),
        ..Log::default()
    };
    let p = parse(
        usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    tune(&p, &HashMap::from([("/w/r".to_string(), log)]), 0)
}

fn pol<'a>(t: &'a Tune, name: &str) -> &'a PolicyResult {
    t.all.policies.iter().find(|p| p.policy == name).unwrap()
}

/// What `touch@1` marks, and the shadow wrote for the same line.
#[test]
fn touch_replay_drops_what_the_shadow_recorded() {
    let usage = line(
        "s1",
        0,
        r#""event":"search","trigger":"hitlist","files":["a.rs"],"ids":["A","B","I"],"feat":{"A":{"kind":"decision","hub":false,"touch":0},"B":{"kind":"decision","hub":false,"touch":1},"I":{"kind":"issue","hub":false,"touch":0}},"would_drop":{"policy":"touch@1","ids":["A"]}"#,
    ) + &cite("s1", 1, "B")
        + &pull("s1", 2, r#"{"id":"A"}"#, "A")
        + &cite("s1", 3, "A");
    let t = run(
        &usage,
        &[("A", "decision"), ("B", "decision"), ("I", "issue")],
    );
    assert_eq!(t.all.sizes.rows_said, 3);
    assert_eq!(t.all.sizes.recorded_would_drop, 1);
    let base = pol(&t, "baseline@1");
    assert_eq!((base.dropped, base.exposure.x, base.exposure.n), (0, 3, 3));
    assert_eq!(base.retained.cited, Rate::new(2, 2));
    let touch = pol(&t, "touch@1");
    // the replay reproduces the shadow: A only (B touched, I an issue)
    assert_eq!(touch.dropped, t.all.sizes.recorded_would_drop);
    assert_eq!((touch.exposure.x, touch.exposure.n), (2, 3));
    assert_eq!(touch.retained.cited, Rate::new(1, 2), "A's cite is lost");
    assert_eq!(touch.retained.pulled, Rate::new(0, 1));
    // the agent pulled the dropped row itself, then cited it
    assert_eq!(touch.retrieved_after_cut.rows, Rate::new(1, 1));
    assert_eq!(touch.retrieved_after_cut.sessions, Rate::new(1, 1));
    assert_eq!(touch.missed_push, Rate::new(1, 1));
    assert_eq!(touch.pushes_silenced, Rate::new(0, 1), "B and I still said");
}

#[test]
fn a_pull_fael_induced_is_no_demand_for_a_dropped_row() {
    let usage = line(
        "s1",
        0,
        r#""event":"search","trigger":"hitlist","files":["a.rs"],"ids":["A"],"said":[{"kind":"count","key":"a.rs|file"}],"feat":{"A":{"kind":"decision","hub":false,"touch":0}}"#,
    ) + &pull("s1", 1, r#"{"files":["a.rs"]}"#, "A")
        + &cite("s1", 2, "A");
    let t = run(&usage, &[("A", "decision")]);
    let touch = pol(&t, "touch@1");
    assert_eq!(touch.dropped, 1);
    assert_eq!(touch.retrieved_after_cut.rows, Rate::new(0, 1));
    assert_eq!(touch.missed_push, Rate::new(0, 1));
    assert_eq!(touch.pushes_silenced, Rate::new(1, 1));
}

#[test]
fn rows_without_a_recorded_working_set_are_not_replayed() {
    // a line from before chunk 3: no `touch` in feat
    let old = line(
        "s0",
        0,
        r#""event":"search","files":["a.rs"],"ids":["A"],"feat":{"A":{"kind":"decision","hub":false}}"#,
    );
    let t = run(
        &(old + &push("s1", 5, "B", 0, "")),
        &[("A", "decision"), ("B", "decision")],
    );
    assert_eq!((t.all.sizes.rows_said, t.all.sizes.evaluable_rows), (2, 1));
    assert_eq!(pol(&t, "touch@1").exposure, Rate::new(0, 1));
}

/// `n` earlier sessions that said A at touch 0 and never used it, each one
/// ended before the next begins.
fn quiet_history(n: u32) -> String {
    (0..n)
        .map(|i| push(&format!("h{i}"), i, "A", 0, ""))
        .collect()
}

#[test]
fn history_holds_a_row_back_only_with_enough_ended_sessions() {
    let k = [("A", "decision")];
    // 5 quiet sessions, then the 6th: under 10% engaged → dropped
    let t = run(&(quiet_history(5) + &push("now", 100, "A", 0, "")), &k);
    assert_eq!(
        pol(&t, "touch-yield@1").dropped,
        1,
        "{:?}",
        pol(&t, "touch-yield@1")
    );
    // 4 is not enough: nothing known, so kept — and touch@1 would drop it
    let t = run(&(quiet_history(4) + &push("now", 100, "A", 0, "")), &k);
    assert_eq!(pol(&t, "touch-yield@1").dropped, 0);
    assert_eq!(
        pol(&t, "touch@1").dropped,
        5,
        "touch@1 drops every untouched said row"
    );
    assert_eq!(
        t.all.fallback_used[4], 5,
        "none of the five had a history level"
    );
}

#[test]
fn a_session_still_running_has_no_outcome_to_learn_from() {
    // five sessions that all stay open past minute 100: none has ended when "now" says A
    let open: String = (0..5)
        .map(|i| push(&format!("h{i}"), i, "A", 0, ""))
        .collect::<String>()
        + &(0..5)
            .map(|i| cite(&format!("h{i}"), 200, "A"))
            .collect::<String>();
    let t = run(
        &(open + &push("now", 100, "A", 0, "")),
        &[("A", "decision")],
    );
    assert_eq!(
        pol(&t, "touch-yield@1").dropped,
        0,
        "no history may leak back"
    );
}

#[test]
fn a_row_with_no_history_falls_back_to_the_class() {
    // A has no history of its own; B, same kind and hub, has five quiet sessions
    let b: String = (0..5)
        .map(|i| push(&format!("h{i}"), i, "B", 0, ""))
        .collect();
    let t = run(
        &(b + &push("now", 100, "A", 0, "")),
        &[("A", "decision"), ("B", "decision")],
    );
    let r = pol(&t, "touch-yield@1");
    assert_eq!(r.dropped, 1, "{r:?}");
    assert_eq!(
        t.all.fallback_used[3], 1,
        "the class level spoke: {:?}",
        t.all.fallback_used
    );
}

#[test]
fn the_decay_window_forgets_old_sessions() {
    // five engaged sessions long ago, five quiet ones since
    let mut u: String = (0..5)
        .map(|i| push(&format!("o{i}"), i, "A", 0, "") + &cite(&format!("o{i}"), i, "A"))
        .collect();
    u += &(5..10)
        .map(|i| push(&format!("q{i}"), i, "A", 0, ""))
        .collect::<String>();
    u += &push("now", 100, "A", 0, "");
    let t = run(&u, &[("A", "decision")]);
    let sweep = |d: Option<usize>| {
        t.all
            .decay_sweep
            .iter()
            .find(|p| p.decay == d)
            .unwrap()
            .result
            .dropped
    };
    // all ten: 50% engaged, kept; the last 20 are still ten
    assert_eq!((sweep(None), sweep(Some(20))), (0, 0));
    // the window itself, below the sweep's smallest value
    let log = Log {
        rows: crate::stats::said::tests::rows(
            "{\"v\":1,\"id\":\"A\",\"ts\":\"2026-10-04T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]}\n",
        ),
        ..Log::default()
    };
    let logs = HashMap::from([("/w/r".to_string(), log)]);
    let p = parse(&u, Path::new("/w/state/usage.jsonl"), &[]);
    let seen = observations(&p, &logs);
    let rows: HashMap<(&str, &str), &Row> = logs
        .iter()
        .flat_map(|(r, l)| l.rows.iter().map(move |x| ((r.as_str(), x.id.as_str()), x)))
        .collect();
    let ob: Vec<Ob> = seen.iter().filter_map(|o| Ob::new(o, &rows)).collect();
    let refs: Vec<&Ob> = ob.iter().collect();
    let dropped = |w| {
        replay::touch_yield(&refs, w)
            .0
            .iter()
            .filter(|d| **d)
            .count()
    };
    assert_eq!((dropped(None), dropped(Some(5))), (0, 1));
}

#[test]
fn strata_split_by_repo_and_client_and_small_ones_are_not_eligible() {
    let other = push("x1", 0, "A", 0, "").replace("\"claude\"", "\"codex\"");
    let t = run(&(push("s1", 0, "A", 0, "") + &other), &[("A", "decision")]);
    assert_eq!(t.strata.len(), 2);
    assert!(
        t.strata.iter().all(|s| !s.eligible),
        "1 session, 1 push each"
    );
    assert_eq!(t.all.sizes.sessions, 2);
    assert_eq!(t.days.as_ref().map(|d| d.0.as_str()), Some("2026-10-05"));
}

#[test]
fn association_reports_the_rate_per_feature_value() {
    let usage = push("s1", 0, "A", 0, "") + &push("s2", 1, "B", 1, "") + &cite("s2", 2, "B");
    let t = run(&usage, &[("A", "decision"), ("B", "decision")]);
    let touch = t
        .all
        .association
        .iter()
        .find(|a| a.feature == "touch")
        .unwrap();
    let g = |v: &str| touch.groups.iter().find(|g| g.value == v).unwrap();
    assert_eq!(
        (g("0").cited, g("1").cited),
        (Rate::new(0, 1), Rate::new(1, 1))
    );
    assert!(touch.phi.is_some());
}
