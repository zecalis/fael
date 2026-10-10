//! `tests/golden/board-v1.json` is the app contract (SPEC-fael-board §10): the SwiftUI app is
//! built against it before `fael board --json` exists. This pins its keys and invariants, so
//! a shape change is a deliberate edit here plus a `v` bump. b3b checks the binary against it.

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
    "merge",
    "fetched_at",
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
];
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

#[test]
fn board_v1_shape() {
    let b = board();
    assert_eq!(b["v"], 1);
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
        assert_eq!(keys(c), CHUNK_KEYS, "{uid}");
        assert!(STATES.contains(&c["state"].as_str().unwrap()), "{uid}");
        if !c["run"].is_null() {
            assert_eq!(keys(&c["run"]), RUN_KEYS, "{uid}");
        }
        let merge = c["merge"].as_str();
        if c["state"] == "review" && !c["run"]["pr"].is_null() {
            assert!(
                matches!(merge, Some("merged" | "open" | "unknown")),
                "{uid}"
            );
            assert!(
                c["fetched_at"].is_string() || merge == Some("unknown"),
                "{uid}"
            );
        } else {
            assert!(
                merge.is_none(),
                "{uid}: merge only on a review chunk with a PR"
            );
        }
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
            "waiting" => c["state"] == "waiting" && c["wait"]["on"] == "owner",
            "review" => c["state"] == "review",
            "ended" => c["state"] == "running" && c["ended"] == true,
            "inbox" => c["state"] == "draft" && c["plan"] == "inbox",
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
