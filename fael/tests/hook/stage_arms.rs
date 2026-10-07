//! PLAN-fael-learn-loop chunk 6, the evaluator on real arm data: the shadow
//! replay promotes, then the candidate/baseline arms promote again or roll
//! back; each repo is judged on its own usage; a stage moves only if its row
//! was filed; usage that was pruned or torn costs nothing; `dup` is written
//! only where self-heal proved the link. Nobody has pinned `touch@1` — every
//! arm line here is synthetic, written the way the push writes it.

use super::stage::{
    ROWS, append, gate_file, gate_rows, probe_repo, put_stage, shadow_usage, shadow_usage_into,
    stage_now, stop_as, stop_look, with_rows,
};
use super::working_set::{add, grep, id_of};
use super::{fael, fael_env, repo, state};
use serde_json::{Value, json};
use std::path::Path;

/// `cand` candidate and `hold` baseline-arm sessions × 8 search pushes over four
/// days, in `d`'s repo. The candidate said nothing (the gate cut every row), the
/// baseline arm said all of them. `dup_c` / `dup_h` sessions of each then filed a
/// row over one they were never shown.
pub(super) fn arm_usage_into(
    d: &Path,
    path: &Path,
    (cand, hold): (usize, usize),
    (dup_c, dup_h): (usize, usize),
) {
    let ids: Vec<String> = (0..ROWS).map(|i| id_of(d, &format!("rule {i}"))).collect();
    let repo = probe_repo(d);
    let feat: serde_json::Map<String, Value> = ids
        .iter()
        .map(|id| {
            let f = json!({"tier": 0, "hub": false, "kind": "decision", "age_d": 0, "touch": 0});
            (id.clone(), f)
        })
        .collect();
    let cut: Vec<Value> = ids
        .iter()
        .map(|id| json!({"id": id, "r": "gate"}))
        .collect();
    let mut text = String::new();
    for (arm, n, dups) in [("candidate", cand, dup_c), ("holdout", hold, dup_h)] {
        for s in 0..n {
            let session = format!("{arm}{s}");
            for p in 0..8 {
                let mut l = json!({
                    "ts": format!("2026-10-0{}T11:{:02}:00.000Z", 1 + s % 4, (s * 8 + p) % 60),
                    "repo": repo, "client": "claude", "session": session, "event": "search",
                    "trigger": "hitlist", "arm": arm, "files": ["src/f0.rs"], "feat": feat,
                    "bytes": 10, "est_tokens": 3,
                });
                if arm == "candidate" {
                    l["policy"] = "touch@1".into();
                    l["ids"] = json!([]);
                    l["cut"] = json!(cut);
                } else {
                    l["policy"] = "baseline@1".into();
                    l["ids"] = json!(ids);
                    l["would_drop"] = json!({"policy": "touch@1", "ids": ids});
                }
                text += &(l.to_string() + "\n");
            }
            if s < dups {
                let o = json!({
                    "ts": "2026-10-04T12:00:00.000Z", "repo": repo, "client": "claude",
                    "session": session, "event": "outcome", "ids": [], "dup": ["X"],
                    "bytes": 0, "est_tokens": 0,
                });
                text += &(o.to_string() + "\n");
            }
        }
    }
    append(path, &text);
}

fn arm_usage(d: &Path, sessions: (usize, usize), dups: (usize, usize)) {
    arm_usage_into(d, &state(d).join("usage.jsonl"), sessions, dups);
}

/// Every `policy:push-gate` row ever filed, superseded ones too: (from, to).
fn gate_history(d: &Path) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut stack = vec![d.join(".git/fael/log"), d.join(".fael/log")];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            for l in std::fs::read_to_string(&path).unwrap_or_default().lines() {
                let Ok(v) = serde_json::from_str::<Value>(l) else {
                    continue;
                };
                if v["key"] != "policy:push-gate" {
                    continue;
                }
                let b: Value = serde_json::from_str(v["text"].as_str().unwrap()).unwrap();
                out.push((
                    b["from"].as_str().unwrap().into(),
                    b["to"].as_str().unwrap().into(),
                ));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn dups(d: &Path) -> Vec<Value> {
    std::fs::read_to_string(state(d).join("usage.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|l| l["event"] == "outcome" && l["client"] == "cli" && l.get("dup").is_some())
        .collect()
}

#[test]
fn the_stages_chain_shadow_to_canary_to_ramp_and_fael_counts_none_of_its_own_rows_as_dup() {
    let d = with_rows();
    shadow_usage(&d, 40, false);
    // two sessions, so the second change's row replaces one the session never saw
    let envs = |s: &'static str| [("FAEL_SESSION", s)];
    stop_as(&state(&d), &d, "e1", &envs("e1"));
    assert_eq!(stage_now(&d).as_deref(), Some("canary"));
    arm_usage(&d, (40, 40), (1, 1)); // 2.5% each way: the dup bar holds
    // past the same-burst window, or the ramp row holds beside the canary one
    // instead of replacing it (parallel adds share a key, 01M47QEJ)
    std::thread::sleep(std::time::Duration::from_millis(1500));
    stop_as(&state(&d), &d, "e2", &envs("e2"));
    assert_eq!(stage_now(&d).as_deref(), Some("ramp"));
    assert_eq!(
        gate_history(&d),
        [
            ("canary".into(), "ramp".into()),
            ("shadow".into(), "canary".into())
        ]
    );
    assert_eq!(gate_rows(&d).len(), 1, "the newest row replaced the first");
    assert!(dups(&d).is_empty(), "{:?}", dups(&d));
    // a ramp that keeps validating stays a ramp and files nothing more
    arm_usage(&d, (40, 40), (1, 1));
    stop_look(&d);
    assert_eq!(stage_now(&d).as_deref(), Some("ramp"));
    assert_eq!(gate_history(&d).len(), 2);
}

#[test]
fn candidate_sessions_that_dup_more_than_the_baseline_arm_roll_the_repo_back() {
    for from in ["canary", "ramp"] {
        let d = with_rows();
        put_stage(&d, "touch@1", from);
        arm_usage(&d, (40, 40), (12, 1)); // 30% against 2.5%
        stop_look(&d);
        assert_eq!(stage_now(&d).as_deref(), Some("baseline"), "{from}");
        let rows = gate_rows(&d);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let body: Value = serde_json::from_str(rows[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            (&body["from"], &body["to"]),
            (&json!(from), &json!("baseline"))
        );
        assert!(
            body["reason"].as_str().unwrap().contains("duplicates"),
            "{body}"
        );
        assert_eq!(body["arm_split"], json!({"candidate": 0, "baseline": 100}));
    }
}

#[test]
fn each_repo_is_judged_on_its_own_usage_in_one_shared_log() {
    let (a, b) = (with_rows(), with_rows());
    let shared = state(&a).join("usage.jsonl");
    // shadow: B's 40 good sessions must not promote A's thin 13
    shadow_usage_into(&a, &shared, 13, false);
    shadow_usage_into(&b, &shared, 40, false);
    stop_as(&state(&a), &a, "ea", &[]);
    assert_eq!(stage_now(&a).as_deref(), Some("shadow"));
    assert!(gate_history(&a).is_empty());
    stop_as(&state(&a), &b, "eb", &[]);
    assert_eq!(stage_now(&b).as_deref(), Some("canary"));
    assert!(gate_history(&a).is_empty(), "B's change is not A's row");
    // arms: A rolls back on its own dups while B, in the same log, ramps
    put_stage(&a, "touch@1", "canary");
    arm_usage_into(&a, &shared, (40, 40), (12, 1));
    arm_usage_into(&b, &shared, (40, 40), (1, 1));
    stop_as(&state(&a), &a, "ea2", &[]);
    stop_as(&state(&a), &b, "eb2", &[]);
    assert_eq!(stage_now(&a).as_deref(), Some("baseline"));
    assert_eq!(stage_now(&b).as_deref(), Some("ramp"));
}

#[test]
fn a_pruned_usage_log_and_a_torn_line_cost_nothing() {
    let d = with_rows();
    // the state remembers a longer log than the one left after a prune
    std::fs::create_dir_all(gate_file(&d).parent().unwrap()).unwrap();
    std::fs::write(
        gate_file(&d),
        json!({"policy": "touch@1", "stage": "shadow", "usage_at": 99_999_999, "pending": 0})
            .to_string(),
    )
    .unwrap();
    shadow_usage(&d, 40, false);
    append(
        &state(&d).join("usage.jsonl"),
        r#"{"event":"search","repo":"#,
    ); // torn, no newline
    stop_look(&d);
    assert_eq!(
        stage_now(&d).as_deref(),
        Some("canary"),
        "counted from the start"
    );
}

#[test]
fn a_plain_folder_keeps_its_stage_beside_its_own_log() {
    let p = std::env::temp_dir().join(format!("fael-plain-{}", fael_core::ulid()));
    std::fs::create_dir_all(p.join("src")).unwrap();
    for i in 0..ROWS {
        add(
            &p,
            "decision",
            &format!("rule {i}"),
            &format!("src/f{i}.rs"),
        );
    }
    assert!(!p.join(".git").exists());
    shadow_usage(&p, 40, false);
    stop_look(&p);
    let file = p.join(".fael/cache/push-gate.json");
    let s: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert_eq!(s["stage"], "canary", "{s}");
    let (_, out, _) = fael(&p, &["tune"], "");
    assert!(out.contains("canary · 50% candidate"), "{out}");
}

#[test]
fn dup_needs_a_session_and_a_row_the_session_was_not_told() {
    let d = repo();
    add(&d, "decision", "old rule", "src/a.rs");
    let old = id_of(&d, "old rule");
    let over = |envs: &[(&str, &str)], text: &str, old: &str| {
        let (ok, _, err) = fael_env(
            &d,
            &[
                "add",
                "decision",
                text,
                "--files",
                "src/a.rs",
                "--supersedes",
                old,
            ],
            "",
            envs,
        );
        assert!(ok, "{err}");
    };
    // outside any session there is nobody to have been shown it
    over(&[], "new rule", &old);
    assert!(dups(&d).is_empty());
    // a compacted context lost what was pushed: the session was told, then not
    add(&d, "decision", "second rule", "src/b.rs");
    let second = id_of(&d, "second rule");
    grep(&d, "s1", &["src/b.rs"]);
    over(&[("FAEL_SESSION", "s1")], "second rule v2", &second);
    assert!(dups(&d).is_empty(), "told, then superseded: no dup");
    add(&d, "decision", "third rule", "src/c.rs");
    let third = id_of(&d, "third rule");
    grep(&d, "s2", &["src/c.rs"]);
    let p = format!(
        r#"{{"cwd":{},"session":"s2","source":"compact"}}"#,
        super::json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "session-start"], &p);
    assert!(ok, "{out}{err}");
    over(&[("FAEL_SESSION", "s2")], "third rule v2", &third);
    assert_eq!(dups(&d).len(), 1, "{:?}", dups(&d));
    assert_eq!(dups(&d)[0]["dup"], json!([third]));
}

/// A log that cannot be written: unix permissions only.
#[cfg(unix)]
mod denied {
    use super::*;
    use std::path::PathBuf;

    fn set_mode(root: &Path, mode: u32) -> bool {
        use std::os::unix::fs::PermissionsExt;
        let mut any = false;
        let mut stack: Vec<PathBuf> = vec![root.join(".git/fael/log"), root.join(".fael/log")];
        while let Some(p) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&p) else {
                continue;
            };
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    stack.push(path.clone());
                }
                let m = if path.is_dir() { mode | 0o111 } else { mode };
                any |= std::fs::set_permissions(&path, std::fs::Permissions::from_mode(m)).is_ok();
            }
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode | 0o111));
        }
        any
    }

    #[test]
    fn no_row_no_change_and_the_next_look_tries_again() {
        let d = with_rows();
        shadow_usage(&d, 40, false);
        assert!(set_mode(&d, 0o444));
        // root writes through any mode: then there is no failure to test
        let probe = d.join(".git/fael/log").join("probe");
        if std::fs::write(&probe, "x").is_ok() {
            let _ = std::fs::remove_file(&probe);
            set_mode(&d, 0o755);
            return;
        }
        stop_look(&d);
        assert_eq!(stage_now(&d), None, "the row could not be written");
        set_mode(&d, 0o755);
        stop_look(&d);
        assert_eq!(stage_now(&d).as_deref(), Some("canary"));
        assert_eq!(gate_history(&d), [("shadow".into(), "canary".into())]);
    }
}
