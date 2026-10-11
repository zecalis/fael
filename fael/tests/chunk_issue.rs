//! `fael chunk add --issue` through the real binary (PLAN-fael-board l2): only an open issue
//! links, the brief prints it with the close rule, and the board shows the fixing chunk —
//! under needs you `issue-open` once it is done while the issue stays open.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// The ledger id `fael add` prints first on stdout.
fn row(d: &Path, args: &[&str]) -> String {
    let (ok, out, err) = fael(d, args);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

fn board(d: &Path) -> Value {
    let (ok, out, err) = fael(d, &["board", "--json"]);
    assert!(ok, "{err}");
    serde_json::from_str(&out).unwrap()
}

fn issue_open(b: &Value) -> Vec<String> {
    b["needs_you"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["kind"] == "issue-open")
        .map(|n| n["uid"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_chunk_fixes_open_issues_and_the_board_follows_it() {
    let d = std::env::temp_dir().join(format!("fael-issue-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    let mut git = Command::new("git");
    assert!(
        git.args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let lag = row(
        &d,
        &[
            "add",
            "issue",
            "The count lags one refresh",
            "--files",
            "a.rs",
        ],
    );
    let old = row(&d, &["add", "issue", "Old bug", "--files", "a.rs"]);
    let pick = row(&d, &["add", "decision", "Use one query", "--files", "a.rs"]);
    assert!(fael(&d, &["close", &old, "fixed long ago"]).0);
    let add = |ids: &str| fael(&d, &["chunk", "add", "fix", "--brief", "b", "--issue", ids]);

    // rejected, naming the id: unknown, closed, not an issue — and nothing is added
    for (bad, says) in [
        ("01ZZZZZZZZ", "no row with id"),
        (old.as_str(), "is closed"),
        (pick.as_str(), "not an issue"),
    ] {
        let (ok, _, err) = add(&format!("{lag},{bad}"));
        assert!(
            !ok && err.contains(says) && err.contains(bad),
            "{bad}: {err}"
        );
    }

    // a unique prefix links the full id
    let (ok, uid, err) = add(&lag[..12]);
    assert!(ok, "{err}");
    let uid = uid.trim();
    let (ok, brief, err) = fael(&d, &["chunk", "start", uid, "--run", "R", "--no-worktree"]);
    assert!(ok, "{err}");
    assert!(
        brief.contains(&format!("> {lag} The count lags one refresh")),
        "{brief}"
    );
    assert!(brief.contains("`fael close <id> \"<why>\"` (done never closes them)"));

    let b = board(&d);
    let p = &b["projects"][0];
    assert_eq!(
        p["chunks"].as_array().unwrap().len(),
        1,
        "a rejected add added nothing"
    );
    assert_eq!(p["chunks"][0]["issues"], json!([lag]));
    assert_eq!(
        p["ledger"]["issues"][0]["chunk"],
        json!({"uid": uid, "state": "running"})
    );
    assert!(issue_open(&b).is_empty(), "running: not yet the owner's");

    // done does not close the issue: the owner sees it until someone does
    let (ok, _, err) = fael(
        &d,
        &["chunk", "done", uid, "fixed", "--pr", "5", "--run", "R"],
    );
    assert!(ok, "{err}");
    assert_eq!(issue_open(&board(&d)), [uid]);
    assert!(fael(&d, &["close", &lag, "the count reads the chunk list"]).0);
    assert!(issue_open(&board(&d)).is_empty());
}
