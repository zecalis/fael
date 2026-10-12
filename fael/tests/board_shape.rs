//! `tests/golden/board-v1.json` is the app contract (SPEC-fael-board §10): the SwiftUI app is
//! built against it before `fael board --json` exists. This pins its keys and invariants, so
//! a shape change is a deliberate edit here; a removed or retyped key also bumps `v` (a new
//! nullable key does not); `tests/board.rs` holds the binary to it.

use serde_json::Value;
use std::collections::HashSet;

const CHUNK_KEYS: &[&str] = &[
    "uid",
    "app",
    "plan",
    "label",
    "title",
    "state",
    "size",
    "model_hint",
    "client_hint",
    "scope",
    "due",
    "pin",
    "ready",
    "blocked_by",
    "unblocks",
    "overlaps",
    "pair",
    "stalled",
    "ended",
    "wait",
    "approval",
    "run",
    "handoff",
    "issues",
    "owner_actions",
];
/// `options`: the answers `chunk wait --option` offers, `[]` = none (issue 01M4NWFS)
const WAIT_KEYS: &[&str] = &["on", "text", "until", "options"];
const PLAN_KEYS: &[&str] = &[
    "app", "name", "title", "area", "kind", "state", "rank", "truth", "source", "spec", "repo",
    "remote", "counts",
];
const RUN_KEYS: &[&str] = &[
    "id",
    "client",
    "session",
    "model",
    "worktree",
    "branch",
    "pr",
    "out",
    "started",
    "last_seen",
    "ended",
    "cost",
];
/// A ledger row (PLAN-fael-board l1); an issue also carries `group` before `signals`, and a
/// linked one its newest fixing `chunk` {uid, state} after `group` (l2).
const ROW_KEYS: &[&str] = &[
    "id", "title", "text", "key", "files", "at", "by", "to", "revisit", "held", "signals",
];
const SIGNALS: &[&str] = &["drifted"];
const STATES: &[&str] = &[
    "draft", "open", "running", "waiting", "review", "done", "replaced", "dropped", "parked",
];

fn keys(v: &Value) -> Vec<&str> {
    v.as_object().unwrap().keys().map(String::as_str).collect()
}

fn board() -> Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/board-v1.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn truth_db(b: &Value, c: &Value) -> bool {
    b["projects"].as_array().unwrap().iter().any(|p| {
        p["plans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|pl| pl["app"] == c["app"] && pl["name"] == c["plan"] && pl["truth"] == "db")
    })
}

/// Open rows by kind; issues in group order (largest group first, `find --groups`).
fn ledger(l: &Value) {
    assert_eq!(keys(l), ["issues", "decisions", "notes"]);
    let mut last = 0;
    for (kind, rows) in l.as_object().unwrap() {
        for r in rows.as_array().unwrap() {
            let mut want = ROW_KEYS.to_vec();
            if kind == "issues" {
                want.insert(10, "group");
                if !r["chunk"].is_null() {
                    want.insert(11, "chunk");
                    assert_eq!(keys(&r["chunk"]), ["uid", "state"], "{r}");
                }
                let g = r["group"].as_u64().unwrap();
                assert!(g == last || g == last + 1, "{r}: groups in order");
                last = g;
            }
            assert_eq!(keys(r), want, "{r}");
            assert_eq!(r["id"].as_str().unwrap().len(), 26, "{r}: a ULID");
            for s in r["signals"].as_array().unwrap() {
                assert!(SIGNALS.contains(&s.as_str().unwrap()), "{r}");
            }
        }
    }
}

/// An open ledger issue names `uid` as its newest fixing chunk, in `done` (l2).
fn fixes_open(b: &Value, uid: &Value) -> bool {
    b["projects"].as_array().unwrap().iter().any(|p| {
        p["ledger"]["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["chunk"]["uid"] == *uid && i["chunk"]["state"] == "done")
    })
}

#[test]
fn board_v1_shape() {
    let b = board();
    assert_eq!(b["v"], 1);
    for p in b["projects"].as_array().unwrap() {
        ledger(&p["ledger"]);
        // `.fael/shared` (PLAN-fael-parallel shared-files): absent = none, never empty
        if let Some(sh) = p.get("shared") {
            let sh = sh.as_array().unwrap();
            assert!(!sh.is_empty() && sh.iter().all(Value::is_string), "{sh:?}");
        }
        for pl in p["plans"].as_array().unwrap() {
            assert_eq!(keys(pl), PLAN_KEYS, "{pl}");
            if matches!(pl["state"].as_str(), Some("done" | "parked")) {
                assert!(
                    !p["chunks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|c| c["plan"] == pl["name"]),
                    "an archived plan lists counts only: {pl}"
                );
            }
        }
    }
    let chunks: Vec<&Value> = b["projects"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| p["chunks"].as_array().unwrap())
        .collect();
    let uids: HashSet<&str> = chunks.iter().map(|c| c["uid"].as_str().unwrap()).collect();
    assert_eq!(uids.len(), chunks.len(), "uid is unique across projects");
    for c in &chunks {
        let uid = c["uid"].as_str().unwrap();
        assert_eq!(uid.len(), 26, "{uid}: a ULID");
        // `conflicts` (PLAN-fael-parallel merge-check): last, and only on an open PR
        let mut k = keys(c);
        if k.last() == Some(&"conflicts") {
            k.pop();
            assert!(
                matches!(c["state"].as_str(), Some("review" | "done")),
                "{uid}"
            );
            assert!(
                c["conflicts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(Value::is_string)
            );
        }
        assert_eq!(k, CHUNK_KEYS, "{uid}");
        assert!(STATES.contains(&c["state"].as_str().unwrap()), "{uid}");
        for (k, want) in [("run", RUN_KEYS), ("wait", WAIT_KEYS)] {
            assert!(c[k].is_null() || keys(&c[k]) == want, "{uid}: {k}");
        }
        assert!(
            c["ready"] == false || truth_db(&b, c),
            "{uid}: only a db plan's chunk is ready (chunk start takes no md chunk)"
        );
        for o in c["overlaps"].as_array().unwrap() {
            assert!(
                uids.contains(o.as_str().unwrap()),
                "{uid}: overlap names a chunk"
            );
        }
    }
    let by_uid = |u: &Value| {
        let u = u.as_str().unwrap();
        *chunks
            .iter()
            .find(|c| c["uid"] == u)
            .unwrap_or_else(|| panic!("{u}: no chunk"))
    };
    for n in b["needs_you"].as_array().unwrap() {
        let c = by_uid(&n["uid"]);
        let ok = match n["kind"].as_str().unwrap() {
            // an md plan's wait names no `on`
            "waiting" => c["state"] == "waiting" && c["wait"]["on"] != "data",
            "review" => c["state"] == "review",
            "ended" => c["state"] == "running" && c["ended"] == true,
            "inbox" => c["state"] == "draft" && c["plan"] == "inbox",
            // a done chunk whose linked issue is still open: the issue names it
            "issue-open" => c["state"] == "done" && fixes_open(&b, &n["uid"]),
            _ => true, // ponytail: an unknown kind is ignored by the app (b7 adds `ledger`)
        };
        assert!(ok, "needs_you {n}");
    }
    for u in b["queue"].as_array().unwrap() {
        assert_eq!(by_uid(u)["ready"], true, "queue {u}");
    }
    for u in b["running"].as_array().unwrap() {
        let c = by_uid(u);
        assert!(
            c["state"] == "running" && c["ended"] == false,
            "running {u}"
        );
    }
    for u in b["blocked"].as_array().unwrap() {
        let c = by_uid(u);
        assert!(c["state"] == "open" && !c["blocked_by"].as_array().unwrap().is_empty());
    }
}
