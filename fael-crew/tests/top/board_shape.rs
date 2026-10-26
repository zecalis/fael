//! `tests/golden/board-v1.json` is the app contract (SPEC-fael-board §10): the SwiftUI app is
//! built against it before `fael board --json` exists. This pins its keys and invariants, so
//! a shape change is a deliberate edit here; a removed or retyped key also bumps `v` (a new
//! nullable key does not — `client_hint`, `run.review` are optional, absent = none);
//! `tests/board.rs` holds the binary to it.

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
    "model",
    "scope",
    "ready",
    "blocked_by",
    "unblocks",
    "overlaps",
    "overlaps_how",
    "pair",
    "stalled",
    "ended",
    "wait",
    "approval",
    "run",
    "handoff",
    "issues",
    "left",
    "left_open",
    "stale",
    "owner_actions",
];
/// `options`: the answers `chunk wait --option` offers, `[]` = none (issue 01M4NWFS)
const WAIT_KEYS: &[&str] = &["on", "text", "until", "options"];
/// `truth` is always `db` (epoch v8): the app gates `chunk add --plan` on it. `parent`
/// (issue 01M51KW0) is the one optional key: an umbrella plan of the same app, one level,
/// absent = top level.
const PLAN_KEYS: &[&str] = &[
    "app",
    "name",
    "title",
    "area",
    "state",
    "truth",
    "counts",
    "owner_actions",
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
/// A ledger row (PLAN-fael-board l1); a linked issue also carries its newest fixing
/// `chunk` {uid, state} before `signals` (l2).
const ROW_KEYS: &[&str] = &[
    "id", "title", "text", "key", "files", "at", "by", "to", "from", "revisit", "held", "signals",
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

/// Open rows by kind; issues, decisions and notes.
fn ledger(l: &Value) {
    assert_eq!(keys(l), ["issues", "decisions", "notes"]);
    for (kind, rows) in l.as_object().unwrap() {
        for r in rows.as_array().unwrap() {
            let mut want = ROW_KEYS.to_vec();
            if kind == "issues" && !r["chunk"].is_null() {
                want.insert(11, "chunk");
                assert_eq!(keys(&r["chunk"]), ["uid", "state"], "{r}");
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

/// A chunk's keys without the optional tail, each checked: `conflicts` (PLAN-fael-parallel
/// merge-check: only on an open PR), then `touched` (the paths `done` claimed from the run's diff: only once claimed).
/// `client_hint` (`chunk add|edit --client`; absent = none, issue 01M5F9DH) is stripped
/// wherever serde put it (after `model`) before the base-keys compare.
fn optional<'a>(c: &'a Value, uid: &str) -> Vec<&'a str> {
    let mut k = keys(c);
    if k.last() == Some(&"touched") {
        k.pop();
        assert!(
            matches!(c["state"].as_str(), Some("review" | "done")),
            "{uid}"
        );
        assert!(
            c["touched"]
                .as_array()
                .unwrap()
                .iter()
                .all(Value::is_string)
        );
    }
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
    // `merged: true` (a review chunk on a branch git says is on the base); absent = not known
    if let Some(i) = k.iter().position(|k| *k == "merged") {
        k.remove(i);
        assert_eq!(c["state"], "review", "{uid}");
        assert_eq!(c["merged"], true, "{uid}");
    }
    if let Some(i) = k.iter().position(|k| *k == "client_hint") {
        k.remove(i);
        assert!(c["client_hint"].is_string(), "{uid}");
    }
    k
}

/// A plan carries `PLAN_KEYS` and, when nested, a `parent` that is a top-level plan of its app.
/// Its `counts` keys are chunk states (`dropped` rides along, issue 01M5F9DH).
fn plan_shape(project: &Value, pl: &Value) {
    let mut k = keys(pl);
    k.retain(|k| *k != "parent");
    assert_eq!(k, PLAN_KEYS, "{pl}");
    for k in pl["counts"].as_object().unwrap().keys() {
        assert!(STATES.contains(&k.as_str()), "{pl}: counts key {k}");
    }
    if let Some(par) = pl.get("parent") {
        let up = project["plans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["app"] == pl["app"] && q["name"] == *par);
        assert!(
            up.is_some_and(|q| q.get("parent").is_none()),
            "a parent is a top-level plan of the same app: {pl}"
        );
    }
}

/// Issue 01M5F9DH: the golden shows every live field once, so the app builds against
/// it — a `client_hint` chunk, a `dropped` count, and a reviewed run.
fn shows_live_fields(b: &Value, chunks: &[&Value]) {
    assert!(
        chunks
            .iter()
            .any(|c| c.get("client_hint").is_some_and(Value::is_string)),
        "golden shows no client_hint chunk"
    );
    assert!(
        b["projects"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|p| p["plans"].as_array().unwrap())
            .any(|pl| pl["counts"].get("dropped").is_some()),
        "golden shows no dropped count"
    );
    assert!(
        chunks
            .iter()
            .any(|c| c["run"].get("review").is_some_and(Value::is_string)),
        "golden shows no reviewed run"
    );
}

/// A chunk's `run`/`wait` objects carry their key sets; `review`
/// (`chunk done --review clean|fixed|failed`; absent = none) is the run's trailing
/// optional, like `client_hint` is the chunk's.
fn subkeys(c: &Value, uid: &str) {
    for (k, want) in [("run", RUN_KEYS), ("wait", WAIT_KEYS)] {
        let mut have = Vec::new();
        if !c[k].is_null() {
            have = keys(&c[k]);
            if k == "run" && have.last() == Some(&"review") {
                have.pop();
                assert!(c["run"]["review"].is_string(), "{uid}");
            }
        }
        assert!(c[k].is_null() || have == want, "{uid}: {k}");
    }
}

#[test]
fn board_v1_shape() {
    let b = board();
    assert_eq!(b["v"], 1);
    models_coherent(&b);
    for p in b["projects"].as_array().unwrap() {
        ledger(&p["ledger"]);
        // `.fael/shared` (PLAN-fael-parallel shared-files): absent = none, never empty
        if let Some(sh) = p.get("shared") {
            let sh = sh.as_array().unwrap();
            assert!(!sh.is_empty() && sh.iter().all(Value::is_string), "{sh:?}");
        }
        // decision 01M4V0FY: every local branch but the base's; absent = none, never empty
        if let Some(br) = p.get("branches") {
            let br = br.as_array().unwrap();
            assert!(!br.is_empty(), "{p}");
            for b in br {
                assert_eq!(keys(b), ["name", "state", "chunks"], "{b}");
                assert!(matches!(
                    b["state"].as_str(),
                    Some("local" | "pushed" | "merged")
                ));
            }
        }
        for pl in p["plans"].as_array().unwrap() {
            plan_shape(p, pl);
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
    shows_live_fields(&b, &chunks);
    for c in &chunks {
        let uid = c["uid"].as_str().unwrap();
        assert_eq!(uid.len(), 26, "{uid}: a ULID");
        let k = optional(c, uid);
        assert_eq!(k, CHUNK_KEYS, "{uid}");
        assert!(STATES.contains(&c["state"].as_str().unwrap()), "{uid}");
        subkeys(c, uid);
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
            // a running chunk whose branch merged while it still runs
            "merged" => c["state"] == "running",
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

/// Models read (run-telemetry t5): every block's totals equal the sum of its
/// groups, and the outer block is the merge of the projects' — so the golden
/// cannot teach totals ≠ groups or outer ≠ merge.
fn models_coherent(b: &Value) {
    let sum_runs = |m: &Value| {
        m["by_model"]
            .as_object()
            .unwrap()
            .values()
            .map(|r| r["runs"].as_u64().unwrap())
            .sum::<u64>()
    };
    let projects = b["projects"].as_array().unwrap();
    for p in projects {
        let m = &p["models"];
        assert_eq!(m["total"].as_u64().unwrap(), sum_runs(m), "{}", p["name"]);
    }
    let outer = &b["models"];
    let total: u64 = projects
        .iter()
        .map(|p| p["models"]["total"].as_u64().unwrap())
        .sum();
    assert_eq!(
        outer["total"].as_u64().unwrap(),
        total,
        "outer is the merge"
    );
    assert_eq!(
        outer["total"].as_u64().unwrap(),
        sum_runs(outer),
        "outer groups sum"
    );
}
