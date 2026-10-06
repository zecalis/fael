//! Self-update at session start (PLAN-fael-auto-update chunk 3): the daily check
//! is stamped and started detached, never awaited; the result it left is said
//! once at the next start; both off-switches silence it. The child itself is
//! tested in `tests/auto_update.rs`.

use super::*;

const ON: &[(&str, &str)] = &[("FAEL_NO_AUTO_UPDATE", "")];

fn start(d: &Path, envs: &[(&str, &str)]) -> String {
    let input = format!(r#"{{"cwd":{}}}"#, json(d));
    // the machine's own config dir never decides a test
    let xdg = state(d).join("home/.config");
    let envs = [&[("XDG_CONFIG_HOME", xdg.to_str().unwrap())][..], envs].concat();
    let (ok, out, err) = fael_env(d, &["hook", "session-start"], &input, &envs);
    assert!(ok, "{err}");
    out
}

fn update_json(d: &Path) -> String {
    std::fs::read_to_string(state(d).join("update.json")).unwrap_or_default()
}

fn seed(d: &Path, body: &str) {
    std::fs::create_dir_all(state(d)).unwrap();
    std::fs::write(state(d).join("update.json"), body).unwrap();
}

#[test]
fn a_day_old_check_is_stamped_once_and_a_fresh_one_is_left_alone() {
    let d = repo();
    start(&d, ON);
    let first = update_json(&d);
    assert!(first.contains("\"checked_at\":"), "{first}");
    start(&d, ON);
    assert_eq!(
        update_json(&d),
        first,
        "a stamp under a day old starts nothing"
    );
}

#[test]
fn both_switches_start_nothing() {
    let d = repo();
    start(&d, &[("FAEL_NO_AUTO_UPDATE", "1")]);
    assert_eq!(update_json(&d), "", "env switch");

    let xdg = state(&d).join("home/.config");
    std::fs::create_dir_all(xdg.join("fael")).unwrap();
    std::fs::write(xdg.join("fael/config.toml"), "auto_update = false\n").unwrap();
    start(&d, ON);
    assert_eq!(update_json(&d), "", "config switch");
}

#[test]
fn the_result_is_said_once_then_taken() {
    let d = repo();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    seed(
        &d,
        &format!(r#"{{"checked_at":{now},"from":"0.29.0","to":"0.30.0","result":"updated"}}"#),
    );
    let out = start(&d, ON);
    assert_eq!(
        out.matches("fael: updated 0.29.0 → 0.30.0 · wiring current")
            .count(),
        1,
        "{out}"
    );
    assert!(
        !update_json(&d).contains("\"updated\""),
        "{}",
        update_json(&d)
    );
    assert!(!start(&d, ON).contains("updated 0.29.0"), "said once");

    seed(
        &d,
        &format!(
            r#"{{"checked_at":{now},"to":"0.30.0","result":"failed","why":"brew failed\nmore","log":"/s/update.log"}}"#
        ),
    );
    let out = start(&d, ON);
    assert!(
        out.contains(
            "auto-update to 0.30.0 failed (brew failed) — run fael upgrade, log: /s/update.log"
        ),
        "{out}"
    );
}

/// A day-old stamp restarts the check, and that rewrite must keep the result
/// the last check left for this very session start to say.
#[test]
fn a_result_survives_the_restamp() {
    let d = repo();
    seed(
        &d,
        r#"{"checked_at":0,"from":"0.29.0","to":"0.30.0","result":"updated"}"#,
    );
    let out = start(&d, ON);
    assert!(out.contains("fael: updated 0.29.0 → 0.30.0"), "{out}");
    assert!(
        !update_json(&d).contains("\"updated\""),
        "{}",
        update_json(&d)
    );
}
