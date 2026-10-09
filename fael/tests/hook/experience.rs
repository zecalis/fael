//! PLAN-fael-experience-loop chunk 4: `fael stats` reads the default
//! branch's `fix:` commits and links them to fael — here by the id the
//! commit body names; the close names a check.

use super::{fael, json, repo};
use std::path::Path;
use std::process::Command;

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(d)
        // after the first usage line, whatever the clock's second
        .env("GIT_COMMITTER_DATE", "2099-01-01T00:00:00Z")
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn stats_links_a_fix_commit_by_the_id_it_names() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "retry loops",
            "--files",
            "src/a.rs",
            "--key",
            "a:retry",
        ],
        "",
    );
    assert!(ok, "{err}");
    let id = &out[..8];
    // a push on the file: the repo's first usage line
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    assert!(fael(&d, &["hook", "read"], &input).0);
    let why = "`a.rs` retried forever → cap at 3; guard `tests/retry.rs`";
    assert!(fael(&d, &["close", "--key", "a:retry", why], "").0);
    git(&d, &["add", "src/a.rs"]);
    let body = format!("fix: cap retries\n\n(fael:{id})");
    git(&d, &["commit", "-q", "-m", &body]);
    git(
        &d,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "fix: nobody filed this",
        ],
    );
    git(
        &d,
        &["commit", "-q", "--allow-empty", "-m", "feat: not a fix"],
    );
    git(&d, &["branch", "-M", "main"]);
    let (ok, out, err) = fael(&d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let e = &v["experience"];
    assert_eq!(
        (&e["fix_commits"], &e["fix_commits_linked"]),
        (&2.into(), &1.into()),
        "{e}"
    );
    assert_eq!(
        (&e["fixed"], &e["closed_with_check"]),
        (&1.into(), &1.into()),
        "{e}"
    );
}

/// PLAN-fael-label chunk 3: each label measure is `{state, num, den, since}`
/// — unmeasurable before anything is closed (never 0%), measured once it is,
/// still measured when a repo path with usage has lost its log (counted apart).
#[test]
fn stats_label_measures_unmeasurable_measured_and_gone() {
    let d = repo();
    let add = ["add", "issue", "retry loops", "--files", "src/a.rs"];
    assert!(fael(&d, &[&add[..], &["--key", "a:retry"]].concat(), "").0);
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    assert!(fael(&d, &["hook", "read"], &input).0);
    let label = || {
        let (ok, out, err) = fael(&d, &["stats", "--json"], "");
        assert!(ok, "{err}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        v["experience"]["label"].clone()
    };
    let since = fael_core::stats::LABEL_SINCE;
    let m = |state: &str, num: u32, den: u32| serde_json::json!({"state": state, "num": num, "den": den, "since": since});
    let l = label();
    assert_eq!(l["close_core"], m("unmeasurable", 0, 0), "{l}");
    assert_eq!(l["key_reuse"], m("measured", 0, 1), "{l}");
    let find_hit = serde_json::json!({"state": "unmeasurable", "num": 0, "den": 0, "since": null});
    assert_eq!(l["find_hit"], find_hit, "{l}");
    let why = "retried forever → cap at 3; guard `tests/retry.rs`";
    assert!(fael(&d, &["close", "--key", "a:retry", why], "").0);
    let l = label();
    assert_eq!(
        (&l["close_core"], &l["guard"]),
        (&m("measured", 1, 1), &m("measured", 1, 1)),
        "{l}"
    );
    let (ok, out, _) = fael(&d, &["stats"], "");
    assert!(ok);
    let line = "  label — close core 1/1 (100%) · guard 1/1 (100%) · key reuse 0/1 (0%) · find hit n/a (usage keeps no missed find)";
    assert!(out.contains(line), "{out}");
    // a removed worktree's usage: the state holds, the path is counted apart
    let usage = std::fs::read_dir(d.join("state"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .unwrap();
    let gone = format!(
        r#"{{"ts":"2099-01-01T00:00:00.000Z","repo":{},"client":"claude","event":"read","ids":[]}}"#,
        json(&d.with_extension("gone"))
    );
    let body = std::fs::read_to_string(&usage).unwrap() + &gone + "\n";
    std::fs::write(&usage, body).unwrap();
    let l = label();
    assert_eq!(l["close_core"], m("measured", 1, 1), "{l}");
    assert_eq!(l["gone_repos"], 1, "{l}");
    let (_, out, _) = fael(&d, &["stats"], "");
    assert!(out.contains("1 repo path(s) gone"), "{out}");
}
