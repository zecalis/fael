//! PLAN-fael-learn-loop chunk 6: with `push_policy` unset a repo runs its own
//! stage machine. The push reads the resolved stage from the state file beside
//! the log; the evaluator at session start moves it from the repo's usage and
//! files a `policy:push-gate` row for every change; a human's pin beats it all.

use super::working_set::{add, grep, id_of};
use super::{fael, fael_env, json, repo, state};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(super) fn gate_file(d: &Path) -> PathBuf {
    d.join(".git/fael/cache/push-gate.json")
}

pub(super) fn put_stage(d: &Path, policy: &str, stage: &str) {
    std::fs::create_dir_all(gate_file(d).parent().unwrap()).unwrap();
    std::fs::write(
        gate_file(d),
        json!({"policy": policy, "stage": stage, "usage_at": 0, "pending": 0}).to_string(),
    )
    .unwrap();
}

pub(super) fn stage_now(d: &Path) -> Option<String> {
    let s = std::fs::read_to_string(gate_file(d)).ok()?;
    serde_json::from_str::<Value>(&s).ok()?["stage"]
        .as_str()
        .map(String::from)
}

/// The arm and policy the last search push of `session` carried.
fn arm(d: &Path, session: &str) -> (String, String) {
    // a fresh seen list, or the repeat push says nothing and writes no line
    let _ = std::fs::remove_dir_all(state(d).join("sessions"));
    grep(d, session, &["src/a.rs"]);
    let usage = std::fs::read_to_string(state(d).join("usage.jsonl")).unwrap();
    let l = usage
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .rfind(|l| l["event"] == "search" && l["session"] == session)
        .expect(&usage);
    (
        l["arm"].as_str().unwrap().into(),
        l["policy"].as_str().unwrap().into(),
    )
}

fn seeded() -> PathBuf {
    let d = repo();
    add(&d, "decision", "keep the parser pure", "src/a.rs");
    d
}

#[test]
fn the_stage_file_decides_the_arm_and_a_pin_or_a_bad_file_cannot_be_overruled_by_it() {
    let (cand, hold, all) = (
        ("candidate".to_string(), "touch@1".to_string()),
        ("holdout".to_string(), "baseline@1".to_string()),
        ("all".to_string(), "baseline@1".to_string()),
    );
    let d = seeded();
    // no file: shadow — the default config still changes nothing
    assert_eq!(arm(&d, "s1"), all);
    // FNV-1a("s1") % 100 = 29 and ("s2") = 96: s1 is in canary's 10%? no — in ramp's 80%
    put_stage(&d, "touch@1", "canary");
    assert_eq!(arm(&d, "s1"), hold, "29 is outside canary's 10%");
    put_stage(&d, "touch@1", "ramp");
    assert_eq!(arm(&d, "s1"), cand);
    assert_eq!(arm(&d, "s2"), hold, "96 is outside ramp's 80%");
    // rolled back, a torn file and another policy version all gate nothing
    put_stage(&d, "touch@1", "baseline");
    assert_eq!(arm(&d, "s1"), all);
    std::fs::write(gate_file(&d), "{not json").unwrap();
    assert_eq!(arm(&d, "s1"), all);
    put_stage(&d, "touch@2", "ramp");
    assert_eq!(arm(&d, "s1"), all, "a new version starts in shadow");
    // a human's pin wins over the stage
    put_stage(&d, "touch@1", "ramp");
    std::fs::write(
        d.join(".fael/config.toml"),
        "push_policy = \"baseline@1\"\n",
    )
    .unwrap();
    assert_eq!(arm(&d, "s1"), all);
    // another repo has its own file: repo B stays in shadow while repo A ramps
    let b = seeded();
    assert_eq!(arm(&b, "s1"), all);
}

pub(super) const ROWS: usize = 6;

/// `sessions` sessions × 8 search pushes over four days, each saying the
/// repo's `ROWS` decisions untouched, in shadow (`arm: all`); with `cite`, each
/// session then cites a row, so the cut would have lost what the agent used.
pub(super) fn shadow_usage(d: &Path, sessions: usize, cite: bool) {
    shadow_usage_into(d, &state(d).join("usage.jsonl"), sessions, cite);
}

/// The repo field of `d`'s usage lines, as its own hook writes it (whatever the
/// temp dir resolves to) — read off a probe push.
pub(super) fn probe_repo(d: &Path) -> Value {
    grep(d, "probe", &["src/f0.rs"]);
    std::fs::read_to_string(state(d).join("usage.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|l| l["event"] == "search")
        .unwrap()["repo"]
        .clone()
}

/// `shadow_usage` for repo `d`, written to any machine-wide usage log.
pub(super) fn shadow_usage_into(d: &Path, path: &Path, sessions: usize, cite: bool) {
    let ids: Vec<String> = (0..ROWS).map(|i| id_of(d, &format!("rule {i}"))).collect();
    let probe = json!({"repo": probe_repo(d)});
    let feat: serde_json::Map<String, Value> = ids
        .iter()
        .map(|id| {
            (
                id.clone(),
                json!({"tier": 0, "hub": false, "kind": "decision", "age_d": 0, "touch": 0}),
            )
        })
        .collect();
    let mut text = String::new();
    for s in 0..sessions {
        for p in 0..8 {
            let mut l = json!({
                "ts": format!("2026-10-0{}T10:{:02}:00.000Z", 1 + s % 4, (s * 8 + p) % 60),
                "repo": probe["repo"], "client": "claude", "session": format!("sess{s}"),
                "event": "search", "trigger": "hitlist", "arm": "all", "policy": "baseline@1",
                "files": ["src/f0.rs"], "ids": ids, "feat": feat,
                "bytes": 10, "est_tokens": 3,
            });
            l["would_drop"] = json!({"policy": "touch@1", "ids": ids});
            text += &(l.to_string() + "\n");
        }
        if cite {
            let o = json!({
                "ts": "2026-10-04T12:00:00.000Z", "repo": probe["repo"], "client": "claude",
                "session": format!("sess{s}"), "event": "outcome", "ids": [], "cited": [ids[0]],
                "bytes": 0, "est_tokens": 0,
            });
            text += &(o.to_string() + "\n");
        }
    }
    append(path, &text);
}

pub(super) fn append(path: &Path, text: &str) {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    std::io::Write::write_all(&mut f, text.as_bytes()).unwrap();
}

pub(super) fn with_rows() -> PathBuf {
    let d = repo();
    for i in 0..ROWS {
        add(
            &d,
            "decision",
            &format!("rule {i}"),
            &format!("src/f{i}.rs"),
        );
    }
    d
}

pub(super) fn session_start(d: &Path) {
    start_as(&state(d), d, "evaluator", &[]);
}

/// A session start of repo `d` whose usage log is `state`'s — two repos feeding
/// one machine-wide log, as real ones do.
pub(super) fn start_as(state: &Path, d: &Path, session: &str, envs: &[(&str, &str)]) {
    let p = format!(r#"{{"cwd":{},"session":"{session}"}}"#, json(d));
    let (ok, out, err) = super::fael_at_env(state, d, &["hook", "session-start"], &p, envs);
    assert!(ok, "{out}{err}");
}

pub(super) fn gate_rows(d: &Path) -> Vec<Value> {
    let (_, out, _) = fael(d, &["find", "--key", "policy:push-gate", "--json"], "");
    out.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

#[test]
fn a_shadow_replay_that_clears_the_bars_promotes_to_canary_and_files_the_row_once() {
    let d = with_rows();
    shadow_usage(&d, 40, false);
    assert_eq!(stage_now(&d), None);
    session_start(&d);
    assert_eq!(stage_now(&d).as_deref(), Some("canary"));
    let rows = gate_rows(&d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let body: Value = serde_json::from_str(rows[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        (&body["policy"], &body["from"], &body["to"]),
        (&json!("touch@1"), &json!("shadow"), &json!("canary"))
    );
    assert_eq!(body["arm_split"], json!({"candidate": 10, "baseline": 90}));
    assert!(body["validation"]["replay"].is_object(), "{body}");
    // nothing new since: the next session start does not look again
    session_start(&d);
    assert_eq!(gate_rows(&d).len(), 1);
    // the repo's stage is on `fael tune`, which writes nothing
    let (_, out, _) = fael(&d, &["tune"], "");
    assert!(
        out.contains("canary · 10% candidate / 90% baseline arm"),
        "{out}"
    );
}

#[test]
fn a_shadow_replay_that_misses_a_bar_rolls_back_for_good() {
    let d = with_rows();
    shadow_usage(&d, 40, true); // the cut would have lost a row every session cited
    session_start(&d);
    assert_eq!(stage_now(&d).as_deref(), Some("baseline"));
    let rows = gate_rows(&d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(
        rows[0]["text"].as_str().unwrap().contains("exposure"),
        "{rows:?}"
    );
    // more evidence does not reopen it: only a new version can
    shadow_usage(&d, 40, false);
    session_start(&d);
    assert_eq!(stage_now(&d).as_deref(), Some("baseline"));
    assert_eq!(gate_rows(&d).len(), 1);
}

#[test]
fn too_little_evidence_holds_the_stage_and_files_nothing() {
    let d = with_rows();
    shadow_usage(&d, 13, false); // 104 pushes: enough to look, not enough to decide
    session_start(&d);
    assert_eq!(stage_now(&d).as_deref(), Some("shadow"));
    assert!(gate_rows(&d).is_empty());
}

#[test]
fn a_pin_is_never_evaluated() {
    let d = with_rows();
    std::fs::write(
        d.join(".fael/config.toml"),
        "push_policy = \"baseline@1\"\n",
    )
    .unwrap();
    shadow_usage(&d, 40, false);
    session_start(&d);
    assert_eq!(stage_now(&d), None);
    assert!(gate_rows(&d).is_empty());
}

#[test]
fn a_row_filed_over_one_the_session_was_never_shown_is_a_dup() {
    let d = repo();
    add(&d, "decision", "shown rule", "src/a.rs");
    add(&d, "decision", "unseen rule", "src/b.rs");
    let (shown, unseen) = (id_of(&d, "shown rule"), id_of(&d, "unseen rule"));
    grep(&d, "s1", &["src/a.rs"]); // says `shown`, never `unseen`
    let over = |old: &str, text: &str, file: &str| {
        let (ok, _, err) = fael_env(
            &d,
            &[
                "add",
                "decision",
                text,
                "--files",
                file,
                "--supersedes",
                old,
            ],
            "",
            &[("FAEL_SESSION", "s1")],
        );
        assert!(ok, "{err}");
    };
    let dups = || -> Vec<Value> {
        std::fs::read_to_string(state(&d).join("usage.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .filter(|l| l["event"] == "outcome" && l.get("dup").is_some())
            .collect()
    };
    over(&shown, "shown rule v2", "src/a.rs");
    assert!(dups().is_empty(), "a row the session was told is no dup");
    over(&unseen, "unseen rule v2", "src/b.rs");
    let l = dups();
    assert_eq!(l.len(), 1, "{l:?}");
    assert_eq!(
        (&l[0]["dup"], &l[0]["session"]),
        (&json!([unseen]), &json!("s1"))
    );
}

#[test]
fn a_canary_with_no_arm_data_yet_holds() {
    let d = with_rows();
    put_stage(&d, "touch@1", "canary");
    shadow_usage(&d, 40, false); // shadow lines only: no candidate or baseline arm in them
    session_start(&d);
    assert_eq!(stage_now(&d).as_deref(), Some("canary"));
    assert!(gate_rows(&d).is_empty());
}

#[test]
fn pushes_are_counted_once_and_add_up_across_looks() {
    let d = with_rows();
    let pending = || -> u64 {
        let s = std::fs::read_to_string(gate_file(&d)).unwrap();
        serde_json::from_str::<Value>(&s).unwrap()["pending"]
            .as_u64()
            .unwrap()
    };
    shadow_usage(&d, 8, false); // 64 pushes and the probe: under the 100 that earn a look
    session_start(&d);
    session_start(&d); // nothing new: not counted twice
    assert_eq!(pending(), 65);
    shadow_usage(&d, 8, false);
    session_start(&d);
    assert_eq!(
        pending(),
        0,
        "130 pushes: looked, and the count starts over"
    );
}
